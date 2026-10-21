"""hf_worker's inpainting, LoRA and super-resolution paths, on tiny random models (no download).

* An inpainting checkpoint (StableDiffusionInpaintPipeline, 9-channel UNet) loads as an
  `image-to-image` model that needs a mask (white = repaint), refuses a plain generation and an
  edit without a mask, and `strength` reaches the pipeline.
* A text-to-image checkpoint inpaints through `AutoPipelineForInpainting.from_pipe`.
* A LoRA (a local .safetensors file made here) changes the picture for the one request that
  asks for it (the same seed gives the plain picture again after it), at a scale; a missing
  file, a non-.safetensors file, a relative path and a scale out of range are refused.
* A Swin2SR checkpoint (`image-super-resolution`) answers an image enlarged by its factor through
  `run_hf`, refuses another scale and a result above the pixel limit.

Argv: <hf_worker.py> <empty dir>. Prints PASS. Run by `python_hf::tests::inpaint_lora_and_super_resolution`
with the hf-cpu environment's Python (HS_PYHF_PYTHON, with PEFT for the LoRA part)."""
import base64
import importlib.util
import io
import json
import os
import sys

import torch
from diffusers import (
    AutoencoderKL, EulerAncestralDiscreteScheduler, StableDiffusionPipeline, UNet2DConditionModel,
)
from PIL import Image
from transformers import CLIPTextConfig, CLIPTextModel, CLIPTokenizer

spec = importlib.util.spec_from_file_location("hf_worker", sys.argv[1])
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)
work = sys.argv[2]
# A request's LoRA file must sit inside a Harness model folder; this test's folder is one.
os.environ["HARNESS_LORA_ROOTS"] = work


def tiny_pipeline(name, class_name, in_channels):
    folder = os.path.join(work, name)
    torch.manual_seed(0)
    UNet2DConditionModel(
        block_out_channels=(32, 64), layers_per_block=1, sample_size=8, in_channels=in_channels,
        out_channels=4, down_block_types=("DownBlock2D", "CrossAttnDownBlock2D"),
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
        "_class_name": class_name, "_diffusers_version": "0.41.0",
        "unet": ["diffusers", "UNet2DConditionModel"], "vae": ["diffusers", "AutoencoderKL"],
        "text_encoder": ["transformers", "CLIPTextModel"],
        "scheduler": ["diffusers", "EulerAncestralDiscreteScheduler"],
        "tokenizer": ["transformers", "CLIPTokenizer"], "safety_checker": [None, None],
        "feature_extractor": [None, None], "requires_safety_checker": False,
    }
    with open(os.path.join(folder, "model_index.json"), "w") as f:
        json.dump(index, f)
    return folder


def load(folder, task):
    args = worker.parse_args(["--model", folder, "--task", task, "--device", "cpu", "--port", "1"])
    model = worker.Model(args)
    model.load()
    assert model.state == "ok", model.error
    return model


def refused(call, needle):
    try:
        call()
    except worker.BadRequest as e:
        assert needle in str(e), (needle, str(e))
    else:
        raise AssertionError(f"not refused: {needle}")


def b64(img):
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode()


start = b64(Image.new("RGB", (64, 64), (200, 30, 30)))
mask_img = Image.new("L", (64, 64), 0)
mask_img.paste(255, (16, 16, 48, 48))
mask = b64(mask_img)
# OpenAI's mask: transparent = edit.
alpha = Image.new("RGBA", (64, 64), (0, 0, 0, 255))
alpha.paste((0, 0, 0, 0), (16, 16, 48, 48))
openai_mask = b64(alpha)

# --- the safety checker of the original SD repos is diffusers' own, other libraries are code -------
checker_dir = os.path.join(work, "checker")
os.makedirs(checker_dir)
for library, ok in (("stable_diffusion", True), ("my_checker", False)):
    with open(os.path.join(checker_dir, "model_index.json"), "w") as f:
        json.dump({"_class_name": "StableDiffusionPipeline", "safety_checker": [library, "StableDiffusionSafetyChecker"]}, f)
    if ok:
        worker.refuse_custom_code(checker_dir)
    else:
        try:
            worker.refuse_custom_code(checker_dir)
        except worker.CustomCodeRefused:
            pass
        else:
            raise AssertionError("another library's component was accepted")

# --- an inpainting checkpoint ------------------------------------------------------------------
inpaint_dir = tiny_pipeline("inpaint", "StableDiffusionInpaintPipeline", 9)
inpaint = load(inpaint_dir, "image-to-image")
assert inpaint.inpaint_only and type(inpaint.pipe).__name__ == "StableDiffusionInpaintPipeline"
assert inpaint.variant("inpaint") is inpaint.pipe
for m in (mask, openai_mask):
    out = worker.edit_image(inpaint, "a", start, m, {"steps": 2, "strength": 0.9, "seed": 3})
    assert len(out) == 1 and out[0].size == (64, 64), out
refused(lambda: worker.edit_image(inpaint, "a", start, None, {"steps": 2}), "send a mask")
refused(lambda: worker.text_to_image(inpaint, "a", {"steps": 2}), "inpaints")

# --- a text-to-image checkpoint inpaints through from_pipe ------------------------------------------
t2i_dir = tiny_pipeline("t2i", "StableDiffusionPipeline", 4)
t2i = load(t2i_dir, "text-to-image")
assert not t2i.inpaint_only
out = worker.edit_image(t2i, "a", start, mask, {"steps": 2, "strength": 0.5, "seed": 3})
assert out[0].size == (64, 64)

# --- LoRA ------------------------------------------------------------------------------------------
lora_dir = os.path.join(work, "lora")
os.makedirs(lora_dir)
try:
    import peft  # noqa: F401
except ImportError:
    peft = None
if peft is not None:
    from peft import LoraConfig
    from peft.utils import get_peft_model_state_dict

    unet = UNet2DConditionModel.from_pretrained(os.path.join(t2i_dir, "unet"))
    unet.add_adapter(LoraConfig(r=4, lora_alpha=4, target_modules=["to_k", "to_q", "to_v", "to_out.0"]))
    torch.manual_seed(1)
    for name, p in unet.named_parameters():
        if "lora_B" in name:
            p.data.normal_(0, 0.5)
    StableDiffusionPipeline.save_lora_weights(
        lora_dir, unet_lora_layers=get_peft_model_state_dict(unet), weight_name="style.safetensors"
    )
    lora = os.path.join(lora_dir, "style.safetensors")
    assert os.path.isfile(lora)

    def draw(params):
        arr = worker.text_to_image(t2i, "a", {"steps": 2, "seed": 5, "width": 64, "height": 64, **params})[0]
        return torch.tensor(list(arr.getdata())).float()

    plain = draw({})
    with_lora = draw({"lora": lora})
    assert (plain - with_lora).abs().max() > 0, "the LoRA changed nothing"
    again = draw({})
    assert (plain - again).abs().max() == 0, "the LoRA stayed loaded after its request"
    half = draw({"lora": lora, "lora_scale": 0.2})
    assert (half - with_lora).abs().max() > 0 and (half - plain).abs().max() > 0
    assert not getattr(t2i.pipe.unet, "peft_config", None), "adapter left on the UNet"
    # The edit and inpaint routes take it too.
    a = worker.edit_image(t2i, "a", start, mask, {"steps": 2, "seed": 2, "lora": lora})
    b = worker.edit_image(t2i, "a", start, mask, {"steps": 2, "seed": 2})
    assert a[0].tobytes() != b[0].tobytes()
    # The route's field checking.
    spec_ = worker.lora_request({"lora": lora, "lora_scale": "0.5"})
    assert spec_ == ("file", lora, None, 0.5), spec_
    assert worker.lora_request({"lora": "owner/name:a/b.safetensors"}) == ("repo", "owner/name", "a/b.safetensors", 1.0)
    assert worker.lora_request({}) is None and worker.lora_request({"lora": "  "}) is None
else:
    print("no PEFT, the LoRA loading is skipped")

refused(lambda: worker.lora_request({"lora": os.path.join(work, "missing.safetensors")}), "not available")
refused(lambda: worker.lora_request({"lora": os.path.join(work, "style.bin")}), ".safetensors")
refused(lambda: worker.lora_request({"lora": "style.safetensors"}), "absolute path")
refused(lambda: worker.lora_request({"lora": "../x/y"}), "repository id")
refused(lambda: worker.lora_request({"lora": "a/b", "lora_scale": 9}), "from -4 to 4")
refused(lambda: worker.lora_request({"lora": "a/b", "lora_scale": "x"}), "must be a number")
refused(lambda: worker.lora_request({"lora_scale": 1}), "needs a lora")
refused(lambda: worker.lora_request({"lora": {"a": 1}}), "lora must be")
refused(lambda: worker.text_to_image(t2i, "a", {"steps": 2, "lora": "owner/not-in-cache"}), "Hugging Face cache")
assert not getattr(t2i.pipe.unet, "peft_config", None)

# --- super-resolution (Swin2SR) -----------------------------------------------------------------
from transformers import AutoImageProcessor, Swin2SRConfig, Swin2SRForImageSuperResolution, Swin2SRImageProcessor

sr_dir = os.path.join(work, "swin2sr")
torch.manual_seed(0)
Swin2SRForImageSuperResolution(Swin2SRConfig(
    image_size=16, embed_dim=16, depths=[2], num_heads=[2], window_size=8, upscale=2, img_range=1.0,
    upsampler="pixelshuffle", resi_connection="1conv",
)).save_pretrained(sr_dir)
Swin2SRImageProcessor().save_pretrained(sr_dir)
sr = load(sr_dir, "image-super-resolution")
small = b64(Image.new("RGB", (20, 12), (10, 120, 200)))  # not a multiple of the window: padded inside
r = worker.run_hf(sr, {"inputs": small}, None)
assert (r["width"], r["height"], r["scale"]) == (40, 24, 2), r
img = Image.open(io.BytesIO(base64.b64decode(r["image"])))
assert img.size == (40, 24), img.size
assert worker.run_hf(sr, {"inputs": small, "parameters": {"scale": 2}}, None)["scale"] == 2
refused(lambda: worker.run_hf(sr, {"inputs": small, "parameters": {"scale": 4}}, None), "upscales by 2")
refused(lambda: worker.run_hf(sr, {"inputs": b64(Image.new("RGB", (4, 4)))}, None), "at least 8")
sr.pipe.max_pixels = 500
refused(lambda: worker.run_hf(sr, {"inputs": small}, None), "Largest image")
refused(lambda: worker.run_hf(sr, {"inputs": "!!notimage"}, None), "")

print("PASS")
