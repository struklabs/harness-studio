"""Harness mflux worker: text to image, image editing and inpainting with one local mflux folder.

    <env python> mflux_worker.py --model <dir> --port <port> [--alias <name>] [--max-seconds N]
        [--lora <file.safetensors> [--lora-scale N]]... [--request-lora-dir <dir>]...

mflux has no server of its own; this worker serves what Harness needs, itself:

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise); every route but
  /health needs "Authorization: Bearer <key>" (constant-time comparison of bytes). The key is
  removed from the environment once read and never logged.
* GET /health ({"status": "ok"}), GET /v1/models (the alias), POST /v1/images/generations and
  POST /v1/images/edits; every other route is 404. Both POST routes take JSON: the fields of
  GENERATION_FIELDS (an edit also EDIT_FIELDS, its images inline as base64), and answer
  {"created", "data": [{"b64_json": <PNG>}]}. Any other field is refused (400).
* Browsers: only the origins in HARNESS_WORKER_ORIGINS get CORS headers; their preflight is
  answered here (it carries no key). Any other Origin is refused (403) before anything else.
* The model is the folder given here (mflux's saved format, `transformer/`, `text_encoder/`,
  `vae/`, `tokenizer/`, FLUX.1's `text_encoder_2/` and `tokenizer_2/` too), loaded at start (one
  copy of the weights for both routes): which model it is comes from its card's `base_model`
  (MODELS), and its family (KINDS) says what /v1/images/edits does with it:
  - FLUX.2 [klein]: editing with up to MAX_EDIT_IMAGES reference images ("strength" optional);
  - Z-Image, FLUX.1 schnell / dev / Krea, Qwen-Image: image to image from one image, "strength"
    (0-1, how much of it is redrawn; default DEFAULT_STRENGTH);
  - FLUX.1 Fill: inpainting one image where its "mask" is white (it draws nothing without an
    image: /v1/images/generations is a 400).
* LoRA adapters (--lora, local safetensors files Harness names; --lora-scale per adapter, 1 by
  default) are applied to the transformer at load. With --request-lora-dir, a request may add
  one LoRA of its own ("lora": a .safetensors file, "lora_scale" -4 to 4): its real path (links
  resolved) must be inside one of those folders (Harness passes its model libraries), anything
  else is a 400. The pipeline is rebuilt with it on the model thread and kept until a request
  asks for another set. It listens on 127.0.0.1 only; Hugging Face Hub access is off; proxy
  variables are cleared; no other request field names a file (edit images are written to a
  private temporary folder and removed after the request).
* A request cannot run away or swamp the Mac:
  - steps (MAX_STEPS), images per request (MAX_IMAGES), sides (SIDE_RANGE, a multiple of 16),
    edit images (MAX_EDIT_IMAGES, each at most MAX_EDIT_SIDE pixels a side and MAX_EDIT_BYTES)
    are bounded; anything outside is a 400;
  - the memory one image takes is estimated before it starts (`memory_need`: measured on an M1
    8 GB) and a size that does not fit this Mac is refused (400) with the largest that does;
    without a size, the largest standard square that fits is used (an edit keeps its image's
    shape);
  - generation stops after --max-seconds of wall-clock time (checked between denoising steps:
    504), and as soon as the client goes away (nothing is answered then).
"""

import argparse
import asyncio
import base64
import binascii
import concurrent.futures
import hmac
import io
import json
import math
import os
import re
import secrets
import shutil
import struct
import sys
import tempfile
import threading
import time

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
for _var in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
    os.environ.pop(_var, None)

KEY = os.environ.pop("HARNESS_WORKER_API_KEY", None) or None
ORIGINS = frozenset(o for o in os.environ.pop("HARNESS_WORKER_ORIGINS", "").split(",") if o)
HOST = "127.0.0.1"
GENERATIONS_PATH = "/v1/images/generations"
EDITS_PATH = "/v1/images/edits"
GENERATION_FIELDS = {"model", "prompt", "n", "seed", "steps", "size", "guidance"}
EDIT_FIELDS = {"image", "strength", "mask"}
# What a request may add when the worker was given --request-lora-dir.
LORA_FIELDS = {"lora", "lora_scale"}
LABEL = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")
SIZE = re.compile(r"^(\d{2,5})x(\d{2,5})$")
# The card's base model -> mflux's model name (the Rust side has the same table:
# `MFLUX_MODELS` in engine/mlx.rs).
MODELS = {
    "black-forest-labs/flux.2-klein-4b": "flux2-klein-4b",
    "black-forest-labs/flux.2-klein-base-4b": "flux2-klein-base-4b",
    "black-forest-labs/flux.2-klein-9b": "flux2-klein-9b",
    "black-forest-labs/flux.2-klein-9b-kv": "flux2-klein-9b-kv",
    "black-forest-labs/flux.2-klein-base-9b": "flux2-klein-base-9b",
    "tongyi-mai/z-image-turbo": "z-image-turbo",
    "tongyi-mai/z-image": "z-image",
    "black-forest-labs/flux.1-schnell": "schnell",
    "black-forest-labs/flux.1-dev": "dev",
    "black-forest-labs/flux.1-krea-dev": "krea-dev",
    "black-forest-labs/flux.1-fill-dev": "dev-fill",
    "qwen/qwen-image": "qwen-image",
}
# mflux's model name -> (family, default steps, memory scale of the transformer's share against
# FLUX.2-klein-4B's). Steps: the models' own recommendations (distilled ones take few).
KINDS = {
    "flux2-klein-4b": ("flux2", 4, 1.0),
    "flux2-klein-base-4b": ("flux2", 28, 1.0),
    "flux2-klein-9b": ("flux2", 4, 1.6),
    "flux2-klein-9b-kv": ("flux2", 4, 1.6),
    "flux2-klein-base-9b": ("flux2", 28, 1.6),
    "z-image-turbo": ("z-image", 9, 1.3),
    "z-image": ("z-image", 28, 1.3),
    "schnell": ("flux1", 4, 2.6),
    "dev": ("flux1", 25, 2.6),
    "krea-dev": ("flux1", 25, 2.6),
    "dev-fill": ("fill", 25, 2.6),
    "qwen-image": ("qwen", 30, 4.5),
}
# How much of an image to image source is redrawn when the request does not say.
DEFAULT_STRENGTH = 0.6
MAX_LORAS = 4
MAX_PROMPT_CHARS = 4000
MAX_STEPS = 50
MAX_IMAGES = 4
SIDE_RANGE = (64, 2048)
MAX_EDIT_IMAGES = 4
MAX_EDIT_SIDE = 4096
MAX_EDIT_BYTES = 16 * 1024 * 1024
# A JSON body: the edit images in base64 and a little more.
MAX_BODY = MAX_EDIT_IMAGES * (MAX_EDIT_BYTES * 4 // 3 + 4) + 64 * 1024
# mflux conditions on a reference image of at most this many pixels (it scales larger ones
# down), so that many tokens is what one costs.
MAX_REFERENCE_AREA = 1024 * 1024
# Squares a request without a size gets: the largest that fits this Mac.
DEFAULT_SIDES = (1024, 768, 512, 384, 256)
DEFAULT_MAX_SECONDS = 600
GB = 1e9

# The thread that loads the model and runs every request (MLX streams belong to the thread that
# made them).
MODEL_THREAD = concurrent.futures.ThreadPoolExecutor(max_workers=1, thread_name_prefix="model")


class Refused(Exception):
    """The request asks for something out of range (400)."""


class TooSlow(Exception):
    """The image was not done within the worker's time limit (504)."""


class Gone(Exception):
    """The client went away: nothing is answered."""


def parser():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--alias", default=None)
    p.add_argument("--max-seconds", type=float, default=DEFAULT_MAX_SECONDS)
    p.add_argument("--lora", action="append", default=[])
    p.add_argument("--lora-scale", action="append", type=float, default=[])
    p.add_argument("--request-lora-dir", action="append", default=[])
    return p


def lora_files(paths, scales):
    """The adapters and their scales: existing .safetensors files (at most MAX_LORAS), a scale
    each from -4 to 4 (1 when not given); a ValueError otherwise."""
    if len(paths) > MAX_LORAS:
        raise ValueError("at most %d LoRA adapters" % MAX_LORAS)
    if len(scales) > len(paths):
        raise ValueError("more --lora-scale than --lora")
    files = []
    for path in paths:
        real = os.path.realpath(path)
        if not real.endswith(".safetensors") or not os.path.isfile(real):
            raise ValueError("a LoRA adapter must be a .safetensors file")
        files.append(real)
    scales = list(scales) + [1.0] * (len(paths) - len(scales))
    if any(not -4.0 <= s <= 4.0 for s in scales):
        raise ValueError("a LoRA scale must be from -4 to 4")
    return files, scales


def request_lora(value, scale, roots):
    """A request's own LoRA -> (real path, scale): a .safetensors file whose real path is inside
    one of `roots` (already real paths); Refused otherwise. The message never echoes the path."""
    if not isinstance(value, str) or not value.strip() or len(value) > 4096:
        raise Refused("lora must be the path of a .safetensors file in a model library")
    real = os.path.realpath(value.strip())
    inside = any(os.path.commonpath([real, root]) == root for root in roots)
    if not inside:
        raise Refused("lora must be a file in one of Harness's model libraries")
    if not real.lower().endswith(".safetensors") or not os.path.isfile(real):
        raise Refused("lora must be an existing .safetensors file")
    scale = 1.0 if scale is None else number(scale, "lora_scale", -4.0, 4.0)
    return real, scale


# ---- the model folder ------------------------------------------------------------------------

def card_list(model_dir, key):
    """A list field of the card's front matter (`key: value` or `- item` lines), lower case."""
    try:
        with open(os.path.join(model_dir, "README.md"), encoding="utf-8") as f:
            lines = f.read().splitlines()
    except OSError:
        return []
    if not lines or lines[0].strip() != "---":
        return []
    items, in_list = [], False
    for line in lines[1:]:
        line = line.rstrip()
        if line.strip() == "---":
            break
        stripped = line.lstrip()
        if in_list and stripped.startswith("- "):
            items.append(stripped[2:].strip().strip("\"'").lower())
        elif line.startswith(key + ":"):
            value = line[len(key) + 1:].strip()
            in_list = not value
            if value:
                items.append(value.strip("\"'").lower())
        else:
            in_list = False
    return items


def model_name(model_dir):
    """mflux's name of the folder's model: its card's first known base model, else None."""
    return next((MODELS[b] for b in card_list(model_dir, "base_model") if b in MODELS), None)


def weights_bytes(model_dir):
    """Bytes of the folder's weights (the safetensors of transformer/, the text encoders, vae/)."""
    total = 0
    for part in ("transformer", "text_encoder", "text_encoder_2", "vae"):
        folder = os.path.join(model_dir, part)
        for name in os.listdir(folder) if os.path.isdir(folder) else ():
            if name.endswith(".safetensors"):
                total += os.path.getsize(os.path.join(folder, name))
    return total


# ---- memory ----------------------------------------------------------------------------------

def tokens(width, height):
    """Image tokens of a width x height image (8x VAE, 2x2 patches)."""
    return (width // 16) * (height // 16)


def reference_tokens(width, height):
    """Tokens a reference image costs once mflux has scaled it to at most MAX_REFERENCE_AREA."""
    area = width * height
    if area > MAX_REFERENCE_AREA:
        scale = math.sqrt(MAX_REFERENCE_AREA / area)
        width, height = round(width * scale), round(height * scale)
    return tokens(width, height)


def memory_need(weights, width, height, refs=(), name="flux2-klein-4b"):
    """Peak bytes one image is expected to take: the weights with MLX's share, what grows with
    the image's tokens (the VAE's attention grows with their square) and a little per reference
    image token. Fitted to FLUX.2-klein-4B 3-bit on an M1 8 GB (3.4 GB of weights; peak MLX
    memory 4.67 / 5.71 / 8.83 GB at 256 / 512 / 768 square, 5.78 GB editing a 512 square image
    at 512); another transformer's share (all but the VAE's square term) is the 4B's times its
    KINDS scale (a 9B: 1.6)."""
    t = tokens(width, height)
    ref = sum(reference_tokens(w, h) for w, h in refs)
    scale = KINDS.get(name, ("", 0, 1.0))[2]
    activations = ((6.8e-4 * t + 1e-4 * ref) * scale + 5.3e-7 * t * t) * GB
    return int(weights * 1.31 + activations)


def memory_budget(info):
    """Bytes an image may take on this Mac (`mx.device_info()`): Metal's recommended working
    set, or three quarters of the memory when that is more (macOS gives MLX more when asked)."""
    return max(int(info.get("max_recommended_working_set_size", 0)),
               int(info.get("memory_size", 0) * 3 // 4))


def snap(width, height):
    """`width` x `height` scaled down to fit SIDE_RANGE's top, each side a multiple of 16."""
    scale = min(1.0, SIDE_RANGE[1] / max(width, height))
    w, h = int(width * scale) // 16 * 16, int(height * scale) // 16 * 16
    return max(w, SIDE_RANGE[0]), max(h, SIDE_RANGE[0])


def fit_size(weights, budget, name, refs=(), shape=None):
    """The size a request without one gets: the largest of DEFAULT_SIDES (squares) or, for an
    edit, the first image's shape scaled down, that fits `budget`; None when even the smallest
    does not."""
    for side in DEFAULT_SIDES:
        if shape:
            w, h = shape
            scale = min(1.0, side / max(w, h))
            w, h = snap(max(int(w * scale), 16), max(int(h * scale), 16))
        else:
            w = h = side
        if memory_need(weights, w, h, refs, name) <= budget:
            return w, h
    return None


def check_fits(weights, budget, name, width, height, refs=()):
    """Refuses a size whose image does not fit `budget`, naming the largest square that does."""
    need = memory_need(weights, width, height, refs, name)
    if need <= budget:
        return
    largest = fit_size(weights, budget, name, refs)
    hint = ("; the largest that fits is %dx%d" % largest) if largest else \
        "; this model does not fit this Mac's memory at any size"
    raise Refused("an image of %dx%d needs about %.1f GB and this Mac has about %.1f GB for it%s"
                  % (width, height, need / GB, budget / GB, hint))


# ---- requests --------------------------------------------------------------------------------

def image_size(b):
    """Width and height of a PNG, JPEG or WebP image from its header, None for anything else."""
    if b[:8] == b"\x89PNG\r\n\x1a\n" and len(b) >= 24:
        return struct.unpack(">II", b[16:24])
    if b[:4] == b"RIFF" and b[8:12] == b"WEBP" and len(b) >= 30:
        kind = b[12:16]
        if kind == b"VP8X":
            return (int.from_bytes(b[24:27], "little") + 1, int.from_bytes(b[27:30], "little") + 1)
        if kind == b"VP8 " and b[23:26] == b"\x9d\x01\x2a":
            w, h = struct.unpack("<HH", b[26:30])
            return w & 0x3FFF, h & 0x3FFF
        if kind == b"VP8L" and b[20:21] == b"\x2f":
            bits = int.from_bytes(b[21:25], "little")
            return (bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1
        return None
    if b[:2] == b"\xff\xd8":
        i = 2
        while i + 9 < len(b):
            if b[i] != 0xFF:
                return None
            marker = b[i + 1]
            if marker in (0xD8, 0x01) or 0xD0 <= marker <= 0xD7:
                i += 2
                continue
            length = struct.unpack(">H", b[i + 2:i + 4])[0]
            if 0xC0 <= marker <= 0xCF and marker not in (0xC4, 0xC8, 0xCC):
                h, w = struct.unpack(">HH", b[i + 5:i + 9])
                return w, h
            i += 2 + length
    return None


def whole(v, field, lo, hi):
    if isinstance(v, bool) or not isinstance(v, int) or not lo <= v <= hi:
        raise Refused("%s must be a whole number from %d to %d" % (field, lo, hi))
    return v


def number(v, field, lo, hi):
    if isinstance(v, bool) or not isinstance(v, (int, float)) or not lo <= v <= hi:
        raise Refused("%s must be a number from %g to %g" % (field, lo, hi))
    return float(v)


def parse_size(v):
    m = SIZE.match(v) if isinstance(v, str) else None
    if not m:
        raise Refused("size must be WIDTHxHEIGHT, like 512x512")
    w, h = int(m.group(1)), int(m.group(2))
    lo, hi = SIDE_RANGE
    if not (lo <= w <= hi and lo <= h <= hi) or w % 16 or h % 16:
        raise Refused("size %s is out of range: both sides from %d to %d and a multiple of 16"
                      % (v, lo, hi))
    return w, h


def decode_image(v, i):
    """An edit image or mask (base64 or a data: URL) -> (bytes, (width, height)); `i` names it
    in errors (its number, or "mask")."""
    if not isinstance(v, str) or not v:
        raise Refused("%s must be a base64 PNG, JPEG or WebP" % i)
    if v.startswith("data:"):
        v = v.partition(",")[2]
    if len(v) > MAX_EDIT_BYTES * 4 // 3 + 4:
        raise Refused("%s is larger than %d MB" % (i, MAX_EDIT_BYTES >> 20))
    try:
        raw = base64.b64decode(v, validate=True)
    except (binascii.Error, ValueError):
        raise Refused("%s is not valid base64" % i) from None
    size = image_size(raw)
    if size is None or min(size) < 16:
        raise Refused("%s is not a PNG, JPEG or WebP image of at least 16x16 pixels" % i)
    if max(size) > MAX_EDIT_SIDE:
        raise Refused("%s is %dx%d; an edit takes at most %d pixels a side"
                      % (i, size[0], size[1], MAX_EDIT_SIDE))
    return raw, size


def image_request(raw, edit, lora_roots=()):
    """Checks a request body -> dict(prompt, n, seed, steps, size, guidance, images, strength,
    mask, lora) (`size` None: the worker picks it; `images`: [(bytes, (w, h))]; `lora`: None or
    (real path, scale), only with `lora_roots`)."""
    try:
        body = json.loads(raw)
    except (ValueError, UnicodeDecodeError):
        raise Refused("the body must be JSON") from None
    if not isinstance(body, dict):
        raise Refused("the body must be a JSON object")
    allowed = GENERATION_FIELDS | (EDIT_FIELDS if edit else set()) | \
        (LORA_FIELDS if lora_roots else set())
    unknown = sorted(set(body) - allowed)
    if unknown:
        raise Refused("unsupported field(s) for this model: %s" % ", ".join(unknown))
    prompt = body.get("prompt")
    if not isinstance(prompt, str) or not prompt.strip():
        raise Refused("prompt is required")
    if len(prompt) > MAX_PROMPT_CHARS:
        raise Refused("prompt is longer than %d characters" % MAX_PROMPT_CHARS)
    seed = body.get("seed")
    if seed is None or seed == -1:
        seed = None
    else:
        seed = whole(seed, "seed", 0, 2 ** 32 - 1)
    out = {
        "prompt": prompt.strip(),
        "n": whole(body.get("n", 1), "n", 1, MAX_IMAGES),
        "seed": seed,
        "steps": None if body.get("steps") is None else whole(body["steps"], "steps", 1, MAX_STEPS),
        "size": None if body.get("size") in (None, "auto") else parse_size(body["size"]),
        "guidance": None if body.get("guidance") is None
        else number(body["guidance"], "guidance", 0.0, 20.0),
        "images": [],
        "strength": None,
        "mask": None,
        "lora": None,
    }
    if body.get("lora") is not None:
        out["lora"] = request_lora(body["lora"], body.get("lora_scale"), lora_roots)
    elif body.get("lora_scale") is not None:
        raise Refused("lora_scale needs a lora")
    if edit:
        images = body.get("image")
        images = [images] if isinstance(images, str) else images
        if not isinstance(images, list) or not images:
            raise Refused("image (the picture to edit) is required")
        if len(images) > MAX_EDIT_IMAGES:
            raise Refused("an edit takes at most %d images" % MAX_EDIT_IMAGES)
        out["images"] = [decode_image(v, "image %d" % (i + 1)) for i, v in enumerate(images)]
        if body.get("strength") is not None:
            out["strength"] = number(body["strength"], "strength", 0.0, 1.0)
        if body.get("mask") is not None:
            out["mask"] = decode_image(body["mask"], "mask")
    return out


def check_kind(req, edit, kind):
    """Refuses what this family does not do: FLUX.1 Fill draws only into an image (an edit with
    one image and a mask), image to image takes one image, only Fill takes a mask."""
    if kind == "fill":
        if not edit:
            raise Refused("this model inpaints: send an image and a mask to /v1/images/edits")
        if req["mask"] is None:
            raise Refused("mask (white where the image is redrawn) is required")
    elif req["mask"] is not None:
        raise Refused("this model does not inpaint with a mask")
    if edit and kind != "flux2" and len(req["images"]) != 1:
        raise Refused("this model takes one image to start from")
    if req["mask"] is not None and req["mask"][1] != req["images"][0][1]:
        raise Refused("the mask must be the size of the image")


# ---- generation ------------------------------------------------------------------------------

class Stop:
    """mflux's in-loop callback: raises between denoising steps once the deadline passed or
    the client went away."""

    def __init__(self, deadline, gone, clock=time.monotonic):
        self.deadline, self.gone, self.clock = deadline, gone, clock

    def check(self):
        if self.gone.is_set():
            raise Gone()
        if self.clock() > self.deadline:
            raise TooSlow()

    def call_in_loop(self, *args, **kwargs):
        self.check()


def make_generate(models, weights, budget, name, max_seconds, render, clock=time.monotonic):
    """`generate(request, edit, gone)` -> list of PNG bytes (blocking). `models` holds the
    pipelines (`stop` gets the in-loop callback), `render(model, request, seed, size, paths,
    mask)` makes one PIL image (`paths`: the edit images written out, `mask`: the mask's)."""
    kind, steps_default, _ = KINDS[name]

    def generate(req, edit, gone):
        check_kind(req, edit, kind)
        lora = req.get("lora")
        # The request's LoRA is loaded with the weights (and counted with them).
        loaded = weights + (os.path.getsize(lora[0]) if lora else 0)
        if getattr(models, "use", None) is not None:
            models.use(lora)
        elif lora:
            raise Refused("this worker takes no LoRA per request")
        sizes = [size for _, size in req["images"]]
        # FLUX.2 [klein] conditions on its images (tokens of their own); the others start from
        # theirs, at the size asked for.
        refs = sizes if kind == "flux2" else []
        size = req["size"]
        if size is None:
            size = fit_size(loaded, budget, name, refs, sizes[0] if edit else None)
            if size is None:
                raise Refused("this model does not fit this Mac's memory at any size")
        check_fits(loaded, budget, name, size[0], size[1], refs)
        stop = Stop(clock() + max_seconds, gone, clock)
        models.stop = stop
        seed = req["seed"] if req["seed"] is not None else secrets.randbelow(2 ** 31)
        folder = tempfile.mkdtemp(prefix="harness-mflux-") if edit else None
        try:
            paths = []
            for i, (raw, _) in enumerate(req["images"]):
                path = os.path.join(folder, "image-%d.img" % i)
                with open(path, "wb") as f:
                    f.write(raw)
                paths.append(path)
            mask = None
            if req["mask"] is not None:
                mask = os.path.join(folder, "mask.img")
                with open(mask, "wb") as f:
                    f.write(req["mask"][0])
            if edit and kind not in ("flux2", "fill") and req["strength"] is None:
                req = dict(req, strength=DEFAULT_STRENGTH)
            pngs = []
            for i in range(req["n"]):
                stop.check()
                image = render(models, dict(req, steps=req["steps"] or steps_default),
                               (seed + i) % 2 ** 32, size, paths, mask)
                buf = io.BytesIO()
                image.save(buf, format="PNG")
                pngs.append(buf.getvalue())
            return pngs
        finally:
            models.stop = None
            if folder:
                shutil.rmtree(folder, ignore_errors=True)

    return generate


# ---- HTTP ------------------------------------------------------------------------------------

def error(message, kind, code):
    return {"error": {"message": message, "type": kind, "code": code}}


async def respond(send, status, payload):
    body = json.dumps(payload).encode()
    await send({"type": "http.response.start", "status": status,
                "headers": [(b"content-type", b"application/json"),
                            (b"content-length", str(len(body)).encode())]})
    await send({"type": "http.response.body", "body": body})


PREFLIGHT = [(b"access-control-allow-methods", b"GET, POST"),
             (b"access-control-allow-headers", b"authorization, content-type"),
             (b"access-control-max-age", b"600")]


def cors(inner):
    """Browser access for ORIGINS only (see the module notes)."""

    async def app(scope, receive, send):
        if scope["type"] != "http":
            return await inner(scope, receive, send)
        origin = dict(scope.get("headers") or []).get(b"origin")
        if origin is None:
            return await inner(scope, receive, send)
        if origin.decode("latin-1") not in ORIGINS:
            return await respond(send, 403, error(
                "Origin not allowed.", "permission_error", "origin_not_allowed"))
        allow = [(b"access-control-allow-origin", origin), (b"vary", b"Origin")]
        if scope["method"] == "OPTIONS":
            await send({"type": "http.response.start", "status": 204,
                        "headers": allow + PREFLIGHT})
            return await send({"type": "http.response.body", "body": b""})

        async def with_cors(message):
            if message["type"] == "http.response.start":
                kept = [(k, v) for k, v in message.get("headers") or []
                        if not k.lower().startswith(b"access-control-") and k.lower() != b"vary"]
                message = dict(message, headers=kept + allow)
            await send(message)

        await inner(scope, receive, with_cors)

    return app


def app_for(generate, alias, run=None, clock=time.time, lora_roots=()):
    """The ASGI app. `generate(request, edit, gone)` (blocking) runs on the model thread (`run`
    replaces that in tests); `gone` is set when the client disconnects meanwhile."""
    expected = ("Bearer %s" % KEY).encode()
    routes = {("GET", "/health"), ("GET", "/v1/models"), ("POST", GENERATIONS_PATH),
              ("POST", EDITS_PATH)}
    one_at_a_time = threading.Lock()

    def locked(*args):
        with one_at_a_time:
            return generate(*args)

    if run is None:
        def run(*args):
            return asyncio.get_running_loop().run_in_executor(MODEL_THREAD, locked, *args)

    async def app(scope, receive, send):
        if scope["type"] == "lifespan":
            while True:
                message = await receive()
                if message["type"] == "lifespan.startup":
                    await send({"type": "lifespan.startup.complete"})
                elif message["type"] == "lifespan.shutdown":
                    await send({"type": "lifespan.shutdown.complete"})
                    return
        if scope["type"] != "http":
            return
        method, route = scope["method"], scope["path"]
        if (method, route) not in routes:
            return await respond(send, 404, error("Not found.", "not_found", "not_found"))
        if route == "/health":
            return await respond(send, 200, {"status": "ok"})
        headers = dict(scope.get("headers") or [])
        if not hmac.compare_digest(headers.get(b"authorization", b""), expected):
            return await respond(send, 401, error(
                "Missing or invalid API key.", "authentication_error", "invalid_api_key"))
        if route == "/v1/models":
            entry = {"id": alias or "default_model", "object": "model", "owned_by": "harness"}
            return await respond(send, 200, {"object": "list", "data": [entry]})
        declared = headers.get(b"content-length", b"")
        if declared.isdigit() and int(declared) > MAX_BODY:
            return await respond(send, 413, error(
                "Request body too large.", "invalid_request_error", "body_too_large"))
        chunks, size, more = [], 0, True
        while more:
            message = await receive()
            if message["type"] == "http.disconnect":
                return
            chunk = message.get("body", b"")
            size += len(chunk)
            if size > MAX_BODY:
                return await respond(send, 413, error(
                    "Request body too large.", "invalid_request_error", "body_too_large"))
            chunks.append(chunk)
            more = message.get("more_body", False)
        edit = route == EDITS_PATH
        try:
            req = image_request(b"".join(chunks), edit, lora_roots)
        except Refused as e:
            return await respond(send, 400, error(str(e) + ".", "invalid_request_error",
                                                  "invalid_request"))
        gone = threading.Event()

        async def watch():
            while (await receive())["type"] != "http.disconnect":
                pass
            gone.set()

        watcher = asyncio.ensure_future(watch())
        try:
            pngs = await run(req, edit, gone)
        except Gone:
            print("mflux_worker: the client went away; the image was stopped", file=sys.stderr,
                  flush=True)
            return
        except Refused as e:
            return await respond(send, 400, error(str(e) + ".", "invalid_request_error",
                                                  "invalid_request"))
        except TooSlow:
            return await respond(send, 504, error(
                "The image took longer than the worker's time limit.", "server_error",
                "image_timeout"))
        except Exception as e:  # mflux's own failure, without the folder's path
            return await respond(send, 500, error("Image generation failed: %s" % type(e).__name__,
                                                  "server_error", "generation_failed"))
        finally:
            watcher.cancel()
        if gone.is_set():
            return
        data = [{"b64_json": base64.b64encode(p).decode("ascii")} for p in pngs]
        await respond(send, 200, {"created": int(clock()), "data": data})

    return app


def main(argv=None):
    if KEY is None:
        sys.exit("mflux_worker: HARNESS_WORKER_API_KEY is required")
    args = parser().parse_args(argv)
    if args.alias is not None and not LABEL.match(args.alias):
        sys.exit("mflux_worker: --alias must be 1-64 letters, digits, '.', '_' or '-'")
    model_dir = os.path.realpath(args.model)
    if not os.path.isdir(model_dir):
        sys.exit("mflux_worker: the model must be a local folder")
    name = model_name(model_dir)
    if name is None:
        sys.exit("mflux_worker: the folder's card names no base model this worker runs")
    try:
        loras, scales = lora_files(args.lora, args.lora_scale)
    except ValueError as e:
        sys.exit("mflux_worker: %s" % e)
    kind = KINDS[name][0]
    roots = tuple(os.path.realpath(d) for d in args.request_lora_dir if os.path.isdir(d))

    import gc
    import mlx.core as mx
    import uvicorn
    from mflux.models.common.config import ModelConfig
    import mlx.nn as nn

    config = ModelConfig.from_name(name)

    class Models:
        """The model's pipelines on one copy of the weights. FLUX.2 [klein]: the edit one is
        loaded, the text-to-image one shares its modules and settings (both are made by the
        same initializer; mflux's edit pipeline cannot draw without a reference image). The
        others draw and start from an image with one pipeline."""

        def __init__(self):
            self.stop = None
            self.extra = None
            self.build(())

        def use(self, lora):
            """Rebuilds the pipelines when a request asks for another LoRA than the loaded one
            (none: the definition's own set again)."""
            extra = (lora,) if lora else ()
            if extra == self.extra:
                return
            if len(loras) + len(extra) > MAX_LORAS:
                raise Refused("at most %d LoRA adapters" % MAX_LORAS)
            self.build(extra)

        def build(self, extra):
            paths = loras + [p for p, _ in extra]
            strengths = scales + [s for _, s in extra]
            load = dict(model_path=model_dir, model_config=config, lora_paths=paths or None,
                        lora_scales=strengths or None)
            # The old copy goes before the new one loads: two would not fit an 8 GB Mac.
            self.editor = self.drawer = None
            gc.collect()
            (mx.clear_cache if hasattr(mx, "clear_cache") else mx.metal.clear_cache)()
            if kind == "flux2":
                from mflux.models.flux2.variants import Flux2Klein, Flux2KleinEdit

                self.editor = Flux2KleinEdit(**load)
                self.drawer = Flux2Klein.__new__(Flux2Klein)
                nn.Module.__init__(self.drawer)
                dict.update(self.drawer, self.editor)
                self.drawer.__dict__.update(self.editor.__dict__)
            else:
                if kind == "z-image":
                    from mflux.models.z_image.variants import ZImage as Pipeline
                elif kind == "flux1":
                    from mflux.models.flux.variants.txt2img.flux import Flux1 as Pipeline
                elif kind == "fill":
                    from mflux.models.flux.variants.fill.flux_fill import Flux1Fill as Pipeline
                else:
                    from mflux.models.qwen.variants.txt2img.qwen_image import QwenImage as Pipeline
                self.editor = self.drawer = Pipeline(**load)
            self.editor.callbacks.register(self)
            self.extra = extra

        def call_in_loop(self, *a, **kw):
            if self.stop is not None:
                self.stop.check()

    def render(models, req, seed, size, paths, mask):
        options = dict(seed=seed, prompt=req["prompt"], num_inference_steps=req["steps"],
                       width=size[0], height=size[1])
        if req["guidance"] is not None:
            options["guidance"] = req["guidance"]
        if not paths:
            return models.drawer.generate_image(**options).image
        if kind == "flux2":
            if req["strength"] is not None:
                options["image_strength"] = req["strength"]
            return models.editor.generate_image(image_paths=paths, **options).image
        if kind == "fill":
            return models.editor.generate_image(image_path=paths[0], masked_image_path=mask,
                                                **options).image
        # mflux's image_strength is how much of the source is kept (the steps it skips);
        # "strength" here is how much is redrawn, as in diffusers.
        return models.editor.generate_image(image_path=paths[0],
                                            image_strength=1.0 - req["strength"],
                                            **options).image

    models = MODEL_THREAD.submit(Models).result()
    budget = memory_budget(mx.device_info() if hasattr(mx, "device_info")
                           else mx.metal.device_info())
    lora_bytes = sum(os.path.getsize(f) for f in loras)
    generate = make_generate(models, weights_bytes(model_dir) + lora_bytes, budget, name,
                             args.max_seconds, render)
    uvicorn.run(cors(app_for(generate, args.alias, lora_roots=roots)), host=HOST, port=args.port,
                log_level="info", access_log=True)


if __name__ == "__main__":
    main()
