"""Offline LoRA SFT worker. Only local models and Harness-owned job directories.

The Rust service owns admission, lifecycle and process cancellation. Trainer checkpoints
include optimizer/RNG state; an interrupted checkpoint is never selected for resume.
"""
import argparse
import json
import os
import shutil
from pathlib import Path

os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("HF_DATASETS_OFFLINE", "1")
os.environ.setdefault("WANDB_DISABLED", "true")


def probe():
    import torch
    import peft  # noqa: F401
    import trl  # noqa: F401
    devices = ["cpu"]
    if torch.cuda.is_available():
        devices.append("cuda")
    if torch.backends.mps.is_available():
        devices.append("mps")
    qlora = False
    reason = "cuda_required"
    if "cuda" in devices:
        try:
            import bitsandbytes as bnb
            layer = bnb.nn.Linear4bit(64, 64, quant_type="nf4").to("cuda")
            with torch.no_grad():
                layer(torch.zeros(1, 64, device="cuda", dtype=torch.float16))
            qlora = True
            reason = "supported"
        except Exception:
            reason = "bitsandbytes_unavailable"
    return {"devices": devices, "qlora": qlora, "qlora_reason": reason}


def last_checkpoint(root):
    candidates = []
    for p in Path(root).glob("checkpoint-*"):
        # trainer_state is written last, after model, optimizer, scheduler and RNG.
        if (p / "trainer_state.json").is_file() and (p / "adapter_model.safetensors").is_file():
            try:
                candidates.append((int(p.name.split("-")[-1]), p))
            except ValueError:
                pass
    return str(max(candidates)[1]) if candidates else None


def train(root, cfg, resume):
    import torch
    from datasets import Dataset
    from peft import LoraConfig, get_peft_model, prepare_model_for_kbit_training
    from transformers import AutoModelForCausalLM, AutoTokenizer, BitsAndBytesConfig, TrainerCallback
    from trl import SFTConfig, SFTTrainer

    torch.set_num_threads(max(1, min(8, os.cpu_count() or 1)))
    device = cfg["device"]
    capabilities = probe()
    if device not in capabilities["devices"]:
        raise ValueError("Selected device is unavailable")
    quantized = cfg["method"] == "qlora"
    if quantized and not capabilities["qlora"]:
        raise ValueError(capabilities["qlora_reason"])
    tokenizer = AutoTokenizer.from_pretrained(cfg["model"], local_files_only=True, trust_remote_code=False)
    if tokenizer.pad_token_id is None:
        tokenizer.pad_token = tokenizer.eos_token
    kwargs = dict(local_files_only=True, trust_remote_code=False,
                  dtype=torch.float32 if device == "cpu" else torch.float16)
    if quantized:
        kwargs.update(quantization_config=BitsAndBytesConfig(load_in_4bit=True,
                      bnb_4bit_quant_type="nf4", bnb_4bit_compute_dtype=torch.float16), device_map={"": 0})
    model = AutoModelForCausalLM.from_pretrained(cfg["model"], **kwargs)
    model.config.use_cache = False
    if quantized:
        model = prepare_model_for_kbit_training(model)
    else:
        model.to(device)
    model = get_peft_model(model, LoraConfig(r=cfg["rank"], lora_alpha=cfg.get("lora_alpha") or cfg["rank"] * 2,
                           target_modules="all-linear", task_type="CAUSAL_LM",
                           lora_dropout=cfg.get("lora_dropout", 0.05)))
    rows = [json.loads(line) for line in (root / "dataset.jsonl").read_text(encoding="utf-8").splitlines() if line.strip()]
    texts = []
    for row in rows:
        messages = row.get("messages")
        if messages is None:
            prompt = row["instruction"] + ("\n" + row["input"] if row.get("input") else "")
            messages = [{"role": "user", "content": prompt}, {"role": "assistant", "content": row["output"]}]
        texts.append({"text": tokenizer.apply_chat_template(messages, tokenize=False, add_generation_prompt=False)})
    # Stable shuffled split; dataset snapshot and seed are unchanged on resume.
    dataset = Dataset.from_list(texts).shuffle(seed=42)
    val_count = max(1, int(len(texts) * cfg["validation_fraction"]))
    val = dataset.select(range(val_count))
    training = dataset.select(range(val_count, len(texts)))

    class Events(TrainerCallback):
        def on_log(self, args, state, control, logs=None, **kwargs):
            logs = logs or {}
            metric = {"step": state.global_step}
            for key in ("loss", "eval_loss"):
                if key in logs:
                    metric[key] = float(logs[key])
            if len(metric) > 1:
                # The Rust service persists these lines (metrics.jsonl) and shows them live.
                print("HS_METRIC " + json.dumps(metric, allow_nan=False), flush=True)

        def on_step_end(self, args, state, control, **kwargs):
            if (root / "cancel").exists():
                control.should_save = True
                control.should_training_stop = True
            return control

    args = SFTConfig(output_dir=str(root), max_steps=cfg["steps"],
                     per_device_train_batch_size=cfg.get("batch_size", 1), per_device_eval_batch_size=1,
                     gradient_accumulation_steps=cfg.get("grad_accum", 1),
                     warmup_ratio=cfg.get("warmup_ratio", 0.0), weight_decay=cfg.get("weight_decay", 0.0),
                     lr_scheduler_type=cfg.get("lr_scheduler", "linear"),
                     learning_rate=cfg["learning_rate"], max_length=cfg["sequence_length"],
                     logging_steps=1, eval_strategy="steps", eval_steps=cfg["eval_steps"],
                     save_strategy="steps", save_steps=cfg["checkpoint_steps"], save_total_limit=2,
                     use_cpu=device == "cpu", fp16=device == "cuda", bf16=False,
                     gradient_checkpointing=False, report_to="none", disable_tqdm=True,
                     dataloader_pin_memory=False, seed=42, optim="adamw_torch")
    trainer = SFTTrainer(model=model, args=args, train_dataset=training, eval_dataset=val,
                         processing_class=tokenizer, callbacks=[Events()])
    print("HS_READY", flush=True)
    checkpoint = last_checkpoint(root) if resume else None
    if resume and not checkpoint:
        raise ValueError("No complete checkpoint is available")
    trainer.train(resume_from_checkpoint=checkpoint)
    if not (root / "cancel").exists():
        trainer.save_model(str(root / "adapter"))
        tokenizer.save_pretrained(str(root / "adapter"))


def export_tokenizer(base, output):
    """Preserve the base tokenizer's format for the runtime-matched converter.

    Re-saving with training's newer transformers can produce tokenizer config fields
    that the converter's transformers cannot read (e.g. extra_special_tokens as a list).
    The tokenizer is not trained, so its original assets are authoritative.
    """
    base, output = Path(base), Path(output)
    output.mkdir(parents=True, exist_ok=True)
    names = {"tokenizer.json", "tokenizer_config.json", "special_tokens_map.json",
             "added_tokens.json", "vocab.json", "vocab.txt", "merges.txt",
             "chat_template.jinja"}
    for source in base.iterdir():
        if source.is_file() and (source.name in names or source.suffix in {".model", ".tiktoken"}):
            shutil.copyfile(source, output / source.name)
    templates = base / "chat_templates"
    if templates.is_dir():
        for source in templates.iterdir():
            if source.is_file() and source.suffix == ".jinja":
                (output / "chat_templates").mkdir(exist_ok=True)
                shutil.copyfile(source, output / "chat_templates" / source.name)


def export(root, cfg, output):
    import torch
    from peft import PeftModel
    from transformers import AutoModelForCausalLM
    model = AutoModelForCausalLM.from_pretrained(cfg["model"], local_files_only=True,
                                                trust_remote_code=False, dtype=torch.float32)
    # A finished job has an adapter; a cancelled or failed one exports its last checkpoint.
    source = root / "adapter"
    if not source.is_dir():
        checkpoint = last_checkpoint(root)
        if not checkpoint:
            raise ValueError("No adapter or checkpoint to export")
        source = Path(checkpoint)
    model = PeftModel.from_pretrained(model, str(source), local_files_only=True)
    model.merge_and_unload().save_pretrained(output, safe_serialization=True)
    export_tokenizer(cfg["model"], output)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", action="store_true")
    parser.add_argument("--job")
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--export")
    args = parser.parse_args()
    if args.probe:
        print(json.dumps(probe()))
        return
    root = Path(args.job)
    cfg = json.loads((root / "job.json").read_text(encoding="utf-8"))["config"]
    if args.export:
        export(root, cfg, args.export)
    else:
        train(root, cfg, args.resume)


if __name__ == "__main__":
    main()
