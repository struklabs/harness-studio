"""hf_worker's diffusers loader against a malicious PyTorch .bin: a pipeline component whose pickle
runs code (here: creates a file) must not load, and the code must not run. Built from scratch
with tiny random components, so no download. Argv: <hf_worker.py> <empty dir>. Prints PASS.
Run by `python_hf::tests::pickle_weights_do_not_run_code` with the hf-cpu environment's
Python (HS_PYHF_PYTHON)."""
import importlib.util
import json
import os
import pickle
import shutil
import sys

import torch
from diffusers import AutoencoderKL, PNDMScheduler, UNet2DConditionModel
from transformers import CLIPTextConfig, CLIPTextModel, CLIPTokenizer

spec = importlib.util.spec_from_file_location("hf_worker", sys.argv[1])
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
# Started with TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD=1: the worker drops it on import.
assert "TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD" not in os.environ
assert "TORCH_FORCE_WEIGHTS_ONLY_LOAD" not in os.environ
work = sys.argv[2]
good = os.path.join(work, "good")

torch.manual_seed(0)
UNet2DConditionModel(
    block_out_channels=(32, 64), layers_per_block=1, sample_size=8, in_channels=4, out_channels=4,
    down_block_types=("DownBlock2D", "CrossAttnDownBlock2D"),
    up_block_types=("CrossAttnUpBlock2D", "UpBlock2D"), cross_attention_dim=32,
).save_pretrained(os.path.join(good, "unet"), safe_serialization=False)
AutoencoderKL(
    block_out_channels=[32], down_block_types=["DownEncoderBlock2D"],
    up_block_types=["UpDecoderBlock2D"], latent_channels=4,
).save_pretrained(os.path.join(good, "vae"), safe_serialization=False)
CLIPTextModel(CLIPTextConfig(
    hidden_size=32, intermediate_size=37, num_attention_heads=4, num_hidden_layers=2,
    vocab_size=1000, projection_dim=32,
)).save_pretrained(os.path.join(good, "text_encoder"))
# transformers 5 always writes safetensors: turn it into the PyTorch .bin older repos ship.
from safetensors.torch import load_file
te = os.path.join(good, "text_encoder")
torch.save(load_file(os.path.join(te, "model.safetensors")), os.path.join(te, "pytorch_model.bin"))
os.remove(os.path.join(te, "model.safetensors"))
PNDMScheduler().save_pretrained(os.path.join(good, "scheduler"))
tok = os.path.join(good, "tokenizer")
os.makedirs(tok)
with open(os.path.join(tok, "vocab.json"), "w") as f:
    json.dump({"<|startoftext|>": 0, "<|endoftext|>": 1, "a</w>": 2}, f)
with open(os.path.join(tok, "merges.txt"), "w") as f:
    f.write("#version: 0.2\n")
CLIPTokenizer(os.path.join(tok, "vocab.json"), os.path.join(tok, "merges.txt")).save_pretrained(tok)
with open(os.path.join(good, "model_index.json"), "w") as f:
    json.dump({
        "_class_name": "StableDiffusionPipeline", "_diffusers_version": "0.41.0",
        "unet": ["diffusers", "UNet2DConditionModel"], "vae": ["diffusers", "AutoencoderKL"],
        "text_encoder": ["transformers", "CLIPTextModel"], "scheduler": ["diffusers", "PNDMScheduler"],
        "tokenizer": ["transformers", "CLIPTokenizer"], "safety_checker": [None, None], "feature_extractor": [None, None],
        "requires_safety_checker": False,
    }, f)

assert worker.pickle_weights(good)
assert worker.torch_loads_weights_only(torch.__version__), torch.__version__
assert not worker.torch_loads_weights_only("2.5.1+cpu") and worker.torch_loads_weights_only("2.6.0")
# Control: the same folder with honest .bin weights loads, so the loader does read the components.
pipe = worker.load_text_to_image(good, torch.float32)
assert type(pipe).__name__ == "StableDiffusionPipeline", type(pipe)

marker = os.path.join(work, "pwned")


class Payload:
    def __reduce__(self):
        return (open, (marker, "w"))


for component, name in (("unet", "diffusion_pytorch_model.bin"), ("text_encoder", "pytorch_model.bin")):
    bad = os.path.join(work, "bad-" + component)
    shutil.copytree(good, bad)
    torch.save({"weight": Payload()}, os.path.join(bad, component, name))
    try:
        worker.load_text_to_image(bad, torch.float32)
    except Exception as e:
        cause = repr(e) + repr(e.__cause__) + repr(e.__context__)
        # transformers raises torch's UnpicklingError; diffusers wraps it in "Unable to load weights".
        if not any(w in cause for w in ("eights only load failed", "Unpickl", "Unable to load weights")):
            sys.exit(f"{component}: refused for another reason: {cause[:300]}")
        print(f"{component}: refused ({type(e).__name__})")
    else:
        sys.exit(f"{component}: a malicious pickle loaded")
    if os.path.exists(marker):
        sys.exit(f"{component}: the pickle ran code")
    try:
        torch.load(os.path.join(bad, component, name), weights_only=True)
        sys.exit(f"{component}: torch.load(weights_only=True) took the pickle")
    except pickle.UnpicklingError:
        pass
    if os.path.exists(marker):
        sys.exit(f"{component}: the pickle ran code")

# The payload is real: an unsafe load of the same file does run it.
torch.load(os.path.join(work, "bad-unet", "unet", "diffusion_pytorch_model.bin"), weights_only=False)
assert os.path.exists(marker), "the test payload did not run under an unsafe load"

# No .bin at all: safetensors only (diffusers is not allowed to fall back to a pickle).
safe = os.path.join(work, "safe")
os.makedirs(safe)
assert not worker.pickle_weights(safe)
print("PASS")
