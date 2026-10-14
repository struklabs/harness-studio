"""hf_worker's image-to-image pipelines: a diffusers folder whose own class starts from an image
(here InstructPix2Pix) loads as that class, edits through itself with only the arguments it
takes, refuses a plain generation and a mask, and a model_index.json naming something other than
a diffusers pipeline is refused. Built from scratch with tiny random components, so no download.
Argv: <hf_worker.py> <empty dir>. Prints PASS. Run by `python_hf::tests::image_to_image_pipelines_edit`
with the hf-cpu environment's Python (HS_PYHF_PYTHON)."""
import base64
import importlib.util
import io
import json
import os
import shutil
import sys

import torch
from diffusers import AutoencoderKL, EulerAncestralDiscreteScheduler, UNet2DConditionModel
from PIL import Image
from transformers import CLIPTextConfig, CLIPTextModel, CLIPTokenizer

spec = importlib.util.spec_from_file_location("hf_worker", sys.argv[1])
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
work = sys.argv[2]
folder = os.path.join(work, "pix2pix")

torch.manual_seed(0)
# InstructPix2Pix: the UNet sees the noisy latents and the start image's latents (8 channels).
UNet2DConditionModel(
    block_out_channels=(32, 64), layers_per_block=1, sample_size=8, in_channels=8, out_channels=4,
    down_block_types=("DownBlock2D", "CrossAttnDownBlock2D"),
    up_block_types=("CrossAttnUpBlock2D", "UpBlock2D"), cross_attention_dim=32,
).save_pretrained(os.path.join(folder, "unet"))
AutoencoderKL(
    block_out_channels=[32], down_block_types=["DownEncoderBlock2D"],
    up_block_types=["UpDecoderBlock2D"], latent_channels=4,
).save_pretrained(os.path.join(folder, "vae"))
CLIPTextModel(CLIPTextConfig(
    hidden_size=32, intermediate_size=37, num_attention_heads=4, num_hidden_layers=2,
    vocab_size=1000, projection_dim=32,
)).save_pretrained(os.path.join(folder, "text_encoder"))
EulerAncestralDiscreteScheduler().save_pretrained(os.path.join(folder, "scheduler"))
tok = os.path.join(folder, "tokenizer")
os.makedirs(tok)
with open(os.path.join(tok, "vocab.json"), "w") as f:
    json.dump({"<|startoftext|>": 0, "<|endoftext|>": 1, "a</w>": 2}, f)
with open(os.path.join(tok, "merges.txt"), "w") as f:
    f.write("#version: 0.2\n")
CLIPTokenizer(
    os.path.join(tok, "vocab.json"), os.path.join(tok, "merges.txt"), model_max_length=77
).save_pretrained(tok)
index = {
    "_class_name": "StableDiffusionInstructPix2PixPipeline", "_diffusers_version": "0.41.0",
    "unet": ["diffusers", "UNet2DConditionModel"], "vae": ["diffusers", "AutoencoderKL"],
    "text_encoder": ["transformers", "CLIPTextModel"],
    "scheduler": ["diffusers", "EulerAncestralDiscreteScheduler"],
    "tokenizer": ["transformers", "CLIPTokenizer"], "safety_checker": [None, None],
    "feature_extractor": [None, None], "image_encoder": [None, None],
    "requires_safety_checker": False,
}
with open(os.path.join(folder, "model_index.json"), "w") as f:
    json.dump(index, f)

args = worker.parse_args(["--model", folder, "--task", "image-to-image", "--device", "cpu", "--port", "1"])
model = worker.Model(args)
model.load()
assert model.state == "ok", model.error
assert type(model.pipe).__name__ == "StableDiffusionInstructPix2PixPipeline", type(model.pipe)
assert model.variant("img2img") is model.pipe

buf = io.BytesIO()
Image.new("RGB", (64, 64), (200, 30, 30)).save(buf, format="PNG")
start = base64.b64encode(buf.getvalue()).decode()
# `strength` is not an InstructPix2Pix argument: it is left out, not an error.
out = worker.edit_image(model, "a", start, None, {"steps": 2, "strength": 0.5, "seed": 1})
assert len(out) == 1 and out[0].size == (64, 64), out

for call in (
    lambda: worker.edit_image(model, "a", start, start, {"steps": 2}),
    lambda: worker.run_hf(model, {"inputs": "a", "parameters": {"steps": 2}}, None),
):
    try:
        call()
    except worker.BadRequest:
        pass
    else:
        raise AssertionError("a mask or a request without an image must be refused")

# Only diffusers' own pipeline classes: anything else in `_class_name` does not load.
for name in ("os.system", "NotAPipeline", "AutoencoderKL", ["custom", "Pipeline"]):
    bad = os.path.join(work, "bad")
    shutil.rmtree(bad, ignore_errors=True)
    shutil.copytree(folder, bad)
    with open(os.path.join(bad, "model_index.json"), "w") as f:
        json.dump({**index, "_class_name": name}, f)
    try:
        worker.load_text_to_image(bad, torch.float32, "image-to-image")
    except worker.CustomCodeRefused:
        pass
    else:
        raise AssertionError(f"{name!r} loaded")

# The other classes the Hub's image-to-image list is made of (too large to build here): each is
# one of diffusers' own pipelines that takes a start image, so the worker edits with the pipeline
# itself. FLUX.2 klein and FLUX.1 Kontext also draw from text (`text-to-image`, loaded through
# AutoPipelineForText2Image); the Edit classes only through their own class (`image-to-image`).
import diffusers
from diffusers.pipelines.auto_pipeline import AUTO_TEXT2IMAGE_PIPELINES_MAPPING

text_to_image = {c.__name__ for c in AUTO_TEXT2IMAGE_PIPELINES_MAPPING.values()}
for name, draws in (
    ("StableDiffusionInstructPix2PixPipeline", False),
    ("StableDiffusionXLImg2ImgPipeline", False),
    ("QwenImageEditPlusPipeline", False),
    ("QwenImageEditPipeline", False),
    ("Flux2KleinPipeline", True),
    ("FluxKontextPipeline", True),
):
    cls = getattr(diffusers, name)
    assert issubclass(cls, diffusers.DiffusionPipeline), name
    assert worker.takes(cls.__new__(cls), "image"), name
    assert (name in text_to_image) == draws, name
# A text-to-image pipeline without a start image argument edits through AutoPipelineForImage2Image.
assert not worker.takes(diffusers.StableDiffusionPipeline.__new__(diffusers.StableDiffusionPipeline), "image")

print("PASS")
