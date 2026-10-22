"""Offline LoRA / QLoRA SFT worker on MLX (Apple Silicon). Only local models and Harness-owned
job directories.

It speaks the same protocol as training_worker.py: `HS_READY` once the model is loaded,
`HS_METRIC {json}` per optimizer step (with the unified memory MLX holds, and greedy answers to
the job's checkpoint prompts at each checkpoint), a `cancel` file asks for a checkpoint and a
stop. `--compare` answers prompts with the base and then with the adapter (`HS_COMPARE`). A
checkpoint holds the adapter, the optimizer state and the data position; `trainer_state.json`
is written last, so an interrupted checkpoint is never resumed from. The adapter is mlx-lm's
format (`adapters.safetensors` + `adapter_config.json`), which `mlx_lm.load(adapter_path=...)`
and `mlx_lm fuse` read.
"""
import argparse
import json
import math
import os
import random
import shutil
from functools import partial
from pathlib import Path

os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")

SEED = 42
# QLoRA on a base model that is not quantized yet: 4-bit linear layers, mlx-lm's group size.
QUANT = {"group_size": 64, "bits": 4}
KEEP_CHECKPOINTS = 2


def lora_keys(model):
    """Module names (inside a block) of the linear layers LoRA adapts: PEFT's `all-linear`
    (attention and MLP projections; embeddings and the LM head stay frozen)."""
    import mlx.nn as nn
    from mlx_lm.models.switch_layers import QuantizedSwitchLinear, SwitchLinear
    linear = (nn.Linear, nn.QuantizedLinear, SwitchLinear, QuantizedSwitchLinear)
    keys = set()
    for layer in model.layers:
        for name, module in layer.named_modules():
            if name and isinstance(module, linear):
                keys.add(name)
    return sorted(keys)


def is_quantized(model):
    import mlx.nn as nn
    return any(isinstance(m, nn.QuantizedLinear) for _, m in model.named_modules())


def load_base(cfg):
    """The base model and tokenizer; QLoRA quantizes a full-precision base in memory."""
    import mlx.nn as nn
    from mlx_lm.utils import load
    model, tokenizer = load(cfg["model"])
    quantized = False
    if cfg["method"] == "qlora" and not is_quantized(model):
        nn.quantize(model, group_size=QUANT["group_size"], bits=QUANT["bits"],
                    class_predicate=lambda _, m: isinstance(m, nn.Linear)
                    and m.weight.shape[-1] % QUANT["group_size"] == 0)
        quantized = True
    return model, tokenizer, quantized


def adapter_config(cfg, keys, quantized):
    rank = cfg["rank"]
    alpha = cfg.get("lora_alpha") or rank * 2
    return {
        "fine_tune_type": "lora",
        # Every block (mlx-lm reads num_layers from the end; -1 is all of them).
        "num_layers": -1,
        "lora_parameters": {"rank": rank, "scale": alpha / rank,
                            "dropout": cfg.get("lora_dropout", 0.05), "keys": keys},
        "model": cfg["model"],
        "harness_method": cfg["method"],
        # Set when the base was quantized for training (QLoRA on a full-precision folder).
        "harness_quantized": QUANT if quantized else None,
    }


def schedule(cfg):
    import mlx.optimizers as optim
    steps, lr = cfg["steps"], cfg["learning_rate"]
    warmup = int(steps * cfg.get("warmup_ratio", 0.0))
    rest = max(1, steps - warmup)
    kind = cfg.get("lr_scheduler", "linear")
    if kind == "cosine":
        main = optim.cosine_decay(lr, rest, 0.0)
    elif kind == "constant":
        main = lambda _: lr  # noqa: E731
    else:
        main = optim.linear_schedule(lr, 0.0, rest)
    if warmup == 0:
        return main
    return optim.join_schedules([optim.linear_schedule(lr / warmup, lr, warmup), main], [warmup])


def rows_to_chats(root, fraction):
    rows = [json.loads(line) for line in (root / "dataset.jsonl").read_text(encoding="utf-8").splitlines()
            if line.strip()]
    chats = []
    for row in rows:
        messages = row.get("messages")
        if messages is None:
            prompt = row["instruction"] + ("\n" + row["input"] if row.get("input") else "")
            messages = [{"role": "user", "content": prompt}, {"role": "assistant", "content": row["output"]}]
        chats.append({"messages": messages})
    # Stable shuffled split; the snapshot and the seed are the same on resume.
    random.Random(SEED).shuffle(chats)
    val_count = max(1, int(len(chats) * fraction))
    return chats[val_count:], chats[:val_count]


def complete_checkpoints(root):
    out = []
    for p in root.glob("checkpoint-*"):
        if (p / "trainer_state.json").is_file() and (p / "adapters.safetensors").is_file():
            try:
                out.append((int(p.name.split("-")[-1]), p))
            except ValueError:
                pass
    return sorted(out)


def memory_metric():
    """Unified memory MLX holds now, and the share of RAM the GPU may use (Metal's recommended
    working set), as the PyTorch worker reports CUDA / MPS memory."""
    import mlx.core as mx
    metric = {"vram_bytes": int(mx.get_active_memory())}
    total = mx.device_info().get("max_recommended_working_set_size")
    if total:
        metric["vram_total_bytes"] = int(total)
    return metric


def generate_samples(model, tokenizer, prompts, max_length):
    """Greedy, bounded answers (the PyTorch worker's limits): up to 128 new tokens, the prompt
    cut to the job's sequence length."""
    from mlx_lm.generate import generate
    was_training = model.training
    model.eval()
    try:
        samples = []
        for prompt in prompts:
            tokens = tokenizer.apply_chat_template([{"role": "user", "content": prompt}],
                                                   add_generation_prompt=True)
            text = generate(model, tokenizer, tokens[-max_length:], max_tokens=128, verbose=False)
            samples.append({"prompt": prompt, "text": text})
        return samples
    finally:
        model.train(was_training)


def save_checkpoint(root, step, micro, model, optimizer, config):
    import mlx.core as mx
    from mlx.utils import tree_flatten
    final = root / f"checkpoint-{step}"
    partial_dir = root / f".checkpoint-{step}.partial"
    shutil.rmtree(partial_dir, ignore_errors=True)
    partial_dir.mkdir(parents=True)
    mx.save_safetensors(str(partial_dir / "adapters.safetensors"), dict(tree_flatten(model.trainable_parameters())))
    mx.save_safetensors(str(partial_dir / "optimizer.safetensors"), dict(tree_flatten(optimizer.state)))
    (partial_dir / "adapter_config.json").write_text(json.dumps(config, indent=1), encoding="utf-8")
    # Written last: its presence marks a complete checkpoint.
    (partial_dir / "trainer_state.json").write_text(json.dumps({"global_step": step, "micro_steps": micro}),
                                                    encoding="utf-8")
    shutil.rmtree(final, ignore_errors=True)
    partial_dir.rename(final)
    for _, old in complete_checkpoints(root)[:-KEEP_CHECKPOINTS]:
        shutil.rmtree(old, ignore_errors=True)


def train(root, cfg, resume):
    import mlx.core as mx
    import mlx.nn as nn
    import mlx.optimizers as optim
    import numpy as np
    from mlx.nn.utils import average_gradients
    from mlx.utils import tree_flatten, tree_map, tree_unflatten
    from mlx_lm.tuner.datasets import CacheDataset, ChatDataset
    from mlx_lm.tuner.trainer import default_loss, evaluate, iterate_batches
    from mlx_lm.tuner.utils import linear_to_lora_layers
    from mlx_lm.utils import maybe_set_recommended_wired_limit

    maybe_set_recommended_wired_limit()
    mx.random.seed(SEED)
    model, tokenizer, quantized = load_base(cfg)
    model.freeze()
    keys = lora_keys(model)
    config = adapter_config(cfg, keys, quantized)
    linear_to_lora_layers(model, config["num_layers"], config["lora_parameters"])

    train_rows, val_rows = rows_to_chats(root, cfg["validation_fraction"])
    batch_size = cfg.get("batch_size", 1)
    if len(train_rows) < batch_size:
        raise ValueError("The dataset has fewer training rows than the batch size")
    train_set = CacheDataset(ChatDataset(train_rows, tokenizer))
    val_set = CacheDataset(ChatDataset(val_rows, tokenizer))
    max_seq = cfg["sequence_length"]

    optimizer = optim.AdamW(learning_rate=schedule(cfg), weight_decay=cfg.get("weight_decay", 0.0))
    accum = max(1, cfg.get("grad_accum", 1))
    start, micro_done = 0, 0
    if resume:
        done = complete_checkpoints(root)
        if not done:
            raise ValueError("No complete checkpoint is available")
        start, last = done[-1]
        model.load_weights(str(last / "adapters.safetensors"), strict=False)
        optimizer.init(model.trainable_parameters())
        optimizer.state = tree_unflatten(list(mx.load(str(last / "optimizer.safetensors")).items()))
        micro_done = json.loads((last / "trainer_state.json").read_text())["micro_steps"]

    loss_and_grad = nn.value_and_grad(model, default_loss)
    state = [model.state, optimizer.state, mx.random.state]

    @partial(mx.compile, inputs=state, outputs=state)
    def micro_step(batch, lengths, prev, update):
        (loss, tokens), grad = loss_and_grad(model, batch, lengths)
        if prev is not None:
            grad = tree_map(lambda a, b: a + b, grad, prev)
        if update:
            grad = average_gradients(grad)
            if accum > 1:
                grad = tree_map(lambda g: g / accum, grad)
            optimizer.update(model, grad)
            grad = None
        return loss, tokens, grad

    batches = iterate_batches(train_set, batch_size, max_seq, loop=True, seed=SEED)
    # A resumed run continues where the data stream stopped.
    for _ in range(micro_done):
        next(batches)

    def eval_loss():
        loss = evaluate(model, val_set, batch_size=1, num_batches=min(len(val_set), 50), max_seq_length=max_seq)
        model.train()
        return loss

    prompts = cfg.get("sample_prompts") or []
    print("HS_READY", flush=True)
    model.train()
    step, micro, grad, losses = start, micro_done, None, []
    while step < cfg["steps"]:
        if (root / "cancel").exists():
            save_checkpoint(root, step, micro, model, optimizer, config)
            return
        for i in range(accum):
            batch, lengths = next(batches)
            loss, _, grad = micro_step(batch, lengths, grad, i == accum - 1)
            mx.eval(state, loss, grad)
            losses.append(loss.item())
            micro += 1
        step += 1
        metric = {"step": step, "loss": float(np.mean(losses)), **memory_metric()}
        losses = []
        if step % cfg["eval_steps"] == 0 or step == cfg["steps"]:
            value = eval_loss()
            if math.isfinite(value):
                metric["eval_loss"] = value
        # The Rust service persists these lines (metrics.jsonl) and shows them live.
        print("HS_METRIC " + json.dumps(metric, allow_nan=False), flush=True)
        if step % cfg["checkpoint_steps"] == 0 and step < cfg["steps"]:
            save_checkpoint(root, step, micro, model, optimizer, config)
        if prompts and (step % cfg["checkpoint_steps"] == 0 or step == cfg["steps"]) \
                and not (root / "cancel").exists():
            samples = generate_samples(model, tokenizer, prompts, max_seq)
            print("HS_METRIC " + json.dumps({"step": step, "samples": samples}, allow_nan=False), flush=True)

    out = root / "adapter.partial"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir()
    mx.save_safetensors(str(out / "adapters.safetensors"), dict(tree_flatten(model.trainable_parameters())))
    (out / "adapter_config.json").write_text(json.dumps(config, indent=1), encoding="utf-8")
    out.rename(root / "adapter")


def adapter_source(root):
    """A finished job's adapter; a cancelled or failed one exports its last checkpoint."""
    if (root / "adapter" / "adapters.safetensors").is_file():
        return root / "adapter"
    done = complete_checkpoints(root)
    if not done:
        raise ValueError("No adapter or checkpoint to export")
    return done[-1][1]


def export_tokenizer(base, output):
    """The base tokenizer's own files over the ones mlx-lm re-saved (the same rule as
    training_worker.py): the GGUF converter's transformers may not read a newer
    tokenizer_config.json, and the tokenizer is not trained."""
    base, output = Path(base), Path(output)
    names = {"tokenizer.json", "tokenizer_config.json", "special_tokens_map.json",
             "added_tokens.json", "vocab.json", "vocab.txt", "merges.txt",
             "chat_template.jinja"}
    for source in base.iterdir():
        if source.is_file() and (source.name in names or source.suffix in {".model", ".tiktoken"}):
            shutil.copyfile(source, output / source.name)


def export(root, cfg, output, dequantize):
    """The base model with the adapter merged in, as an MLX model folder. `dequantize` writes
    full-precision weights (what the GGUF converter reads)."""
    from mlx.utils import tree_unflatten
    from mlx_lm.utils import dequantize_model, load, save
    model, tokenizer, config = load(cfg["model"], adapter_path=str(adapter_source(root)), return_config=True)
    fused = [(n, m.fuse(dequantize=dequantize)) for n, m in model.named_modules() if hasattr(m, "fuse")]
    if fused:
        model.update_modules(tree_unflatten(fused))
    if dequantize:
        model = dequantize_model(model)
        config.pop("quantization", None)
        config.pop("quantization_config", None)
    save(Path(output), cfg["model"], model, tokenizer, config, donate_model=False)
    export_tokenizer(cfg["model"], output)


def compare(root, cfg):
    """The same prompts answered by the base model, then with the run's adapter (`root/adapter`,
    a snapshot the service copied). One model at a time, so an 8 GB Mac holds a single copy."""
    import mlx.core as mx
    from mlx_lm.utils import load
    prompts = json.loads((root / "prompts.json").read_text(encoding="utf-8"))
    model, tokenizer = load(cfg["model"])
    base = generate_samples(model, tokenizer, prompts, cfg["sequence_length"])
    del model
    mx.clear_cache()
    model, tokenizer = load(cfg["model"], adapter_path=str(root / "adapter"))
    trained = generate_samples(model, tokenizer, prompts, cfg["sequence_length"])
    rows = [{"prompt": a["prompt"], "base": a["text"], "trained": b["text"]}
            for a, b in zip(base, trained)]
    print("HS_COMPARE " + json.dumps(rows, allow_nan=False), flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--job", required=True)
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--export")
    parser.add_argument("--dequantize", action="store_true")
    parser.add_argument("--compare", action="store_true")
    args = parser.parse_args()
    root = Path(args.job)
    cfg = json.loads((root / "job.json").read_text(encoding="utf-8"))["config"]
    if args.compare:
        compare(root, cfg)
    elif args.export:
        export(root, cfg, args.export, args.dequantize)
    else:
        train(root, cfg, args.resume)


if __name__ == "__main__":
    main()
