"""Harness Studio python-hf worker: serves one Hugging Face model folder with a transformers
pipeline (or a diffusers text-to-image pipeline) over HTTP.

Started by Harness from its managed environment (`<data>/bin/pyenv/envs/hf-*`), one process per
model, on a loopback port or a Unix socket (Linux runs it without network, see
`pyenv::isolation`). The model is a local folder: nothing is downloaded and remote code is never
run (`trust_remote_code` is always off; a folder that asks for custom code is refused before
loading).

Endpoints
  GET  /health                     {"status":"ok"} | 503 {"status":"loading"} | 500 {"status":"error"}
  GET  /v1/models
  POST /hf                         Hugging Face Inference API: {"inputs": ..., "parameters": {...}}
  POST /v1/chat/completions        text-generation, image-text-to-text (OpenAI shape, SSE streaming,
                                   tools, images as data: or http(s) URLs)
  POST /v1/embeddings              feature-extraction, sentence-similarity (OpenAI shape)
  POST /v1/rerank                  text-ranking (cross-encoders)
  POST /v1/audio/transcriptions    automatic-speech-recognition (multipart "file", OpenAI shape; a
                                   CTranslate2 Whisper export runs on faster-whisper and also answers
                                   language, segments and word timestamps)
  POST /v1/audio/speech            text-to-speech / text-to-audio ({"input": ...} -> audio/wav)
  POST /v1/images/generations      text-to-image (diffusers; OpenAI shape, b64_json)
  POST /v1/images/edits            image-to-image, or inpainting with a mask, on the same pipeline

Errors use the OpenAI shape {"error": {"message", "type", "code"}}. Every route but /health needs
"Authorization: Bearer <HARNESS_WORKER_API_KEY>"; the worker refuses to start without that key.
Browsers: only the origins in HARNESS_WORKER_ORIGINS (the Harness window and the user's CORS
origins) get CORS headers and a preflight answer; any other Origin is refused with 403.
"""

from __future__ import annotations

import argparse
import base64
import io
import ipaddress
import json
import os
import re
import sys
import threading
import time
import traceback
import uuid

# torch.load's weights_only protection is not up to the environment: Harness spawns the worker
# without these, and a worker started some other way drops them before torch is imported.
for _var in ("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", "TORCH_FORCE_WEIGHTS_ONLY_LOAD"):
    os.environ.pop(_var, None)

# --- arguments -------------------------------------------------------------------------------

TEXT_TASKS = {
    "text-classification",
    "token-classification",
    "zero-shot-classification",
    "fill-mask",
    "question-answering",
    "table-question-answering",
    "summarization",
    "translation",
    "text2text-generation",
    "feature-extraction",
    "sentence-similarity",
    "text-ranking",
}
IMAGE_TASKS = {
    "image-classification",
    "zero-shot-image-classification",
    "object-detection",
    "zero-shot-object-detection",
    "image-segmentation",
    "depth-estimation",
    "image-to-text",
    "image-feature-extraction",
    "document-question-answering",
    "visual-question-answering",
    "mask-generation",
}
AUDIO_TASKS = {
    "automatic-speech-recognition",
    "audio-classification",
    "zero-shot-audio-classification",
    "text-to-speech",
    "text-to-audio",
}
# image-to-image: a pipeline that starts from an image (InstructPix2Pix, an Img2Img or Edit class).
VIDEO_TASKS = {"text-to-video", "image-to-video"}
DIFFUSERS_TASKS = {"text-to-image", "image-to-image"} | VIDEO_TASKS
# Chat checkpoints (transformers Auto classes, OpenAI /v1/chat/completions).
CHAT_TASKS = {"text-generation", "image-text-to-text"}
TASKS = TEXT_TASKS | IMAGE_TASKS | AUDIO_TASKS | DIFFUSERS_TASKS | CHAT_TASKS | {"video-text-to-text"}

# Largest request body (an image or a few minutes of audio fit easily).
MAX_BODY_BYTES = 64 * 1024 * 1024

# Upper bounds of the request parameters that decide how much work one request is: larger values
# are refused, not clamped, so a client learns the limit.
PARAM_LIMITS = {
    "width": 2048,
    "height": 2048,
    "num_inference_steps": 100,
    "steps": 100,
    "guidance_scale": 100,
    "cfg_scale": 100,
    "strength": 1,
    "num_images_per_prompt": 4,
    "n": 4,
    "max_new_tokens": 2048,
    "max_length": 4096,
    "num_beams": 8,
    "top_k": 1000,
    "max_answer_len": 512,
    "max_seq_len": 4096,
    "doc_stride": 4096,
    "chunk_length_s": 60,
}


def check_params(params):
    """The request parameters, with the ones that size the work checked against
    `PARAM_LIMITS`; private names (`_...`) and nested objects other than lists are refused."""
    if not isinstance(params, dict):
        raise BadRequest('"parameters" must be an object')
    out = {}
    for k, v in params.items():
        if not isinstance(k, str) or k.startswith("_"):
            raise BadRequest(f"parameter {k!r} is not accepted")
        if isinstance(v, dict) and k != "generate_kwargs":
            raise BadRequest(f"parameter {k!r} must not be an object")
        limit = PARAM_LIMITS.get(k)
        if limit is not None and v is not None:
            try:
                n = float(v)
            except (TypeError, ValueError):
                raise BadRequest(f"parameter {k!r} must be a number")
            if n < 0 or n > limit:
                raise BadRequest(f"parameter {k!r} must be between 0 and {limit}")
        out[k] = v
    gen = out.get("generate_kwargs")
    if isinstance(gen, dict):
        out["generate_kwargs"] = check_params(gen)
    return out


# Tasks whose transformers pipeline name differs from the Hub tag.
PIPELINE_NAME = {
    "sentence-similarity": "feature-extraction",
    "text-ranking": "text-classification",
    "text-to-speech": "text-to-audio",
}


def parse_args(argv=None):
    # No abbreviations: `--ud`, `--trust` and the like must not reach a real flag.
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True, help="local model folder")
    p.add_argument("--task", required=True, choices=sorted(TASKS))
    p.add_argument("--alias", default=None)
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=0)
    p.add_argument("--uds", default=None, help="listen on this Unix socket instead of a port")
    p.add_argument("--device", default="auto", choices=["auto", "cpu", "cuda", "mps"])
    p.add_argument("--dtype", default="auto", choices=["auto", "float32", "float16", "bfloat16"])
    p.add_argument("--threads", type=int, default=0)
    # Diffusers: the most output pixels one request may ask for (width x height x images, in
    # megapixels; 0 = 1 on the CPU, 4 on a GPU) and memory-saving attention / VAE (auto = on the
    # CPU and Apple's GPU).
    p.add_argument("--max-megapixels", type=float, default=0)
    p.add_argument("--low-memory", default="auto", choices=["auto", "on", "off"])
    args = p.parse_args(argv)
    if args.uds is None:
        if not ipaddress.ip_address(args.host).is_loopback:
            p.error("the worker only listens on a loopback address")
        if args.port <= 0:
            p.error("--port or --uds is required")
    if not os.path.isdir(args.model):
        p.error(f"not a folder: {args.model}")
    args.alias = args.alias or os.path.basename(os.path.normpath(args.model))
    return args


# --- model ---------------------------------------------------------------------------------------


class Model:
    """The one model of this process, loaded in the background."""

    def __init__(self, args):
        self.args = args
        self.state = "loading"
        self.error = None
        # The server (`main`) exits on a load error; a test that calls `load()` reads `state`.
        self.exit_on_error = False
        self.pipe = None
        self.st = None  # sentence-transformers model (embeddings / rerank), when the folder is one
        self.ct2 = None  # faster-whisper WhisperModel, when the folder is a CTranslate2 Whisper export
        self.lock = threading.Lock()  # pipelines are not thread-safe: one request at a time
        self.loaded_at = None
        # Diffusers: the folder's own scheduler, the samplers built from it and the image-to-image
        # / inpainting pipelines made from the loaded one (same components, no extra memory).
        self.default_scheduler = None
        self.schedulers = {}
        self.variants = {}

    def max_pixels(self):
        mp = self.args.max_megapixels or (1.0 if self.device() == "cpu" else 4.0)
        return int(mp * 1_000_000)

    def variant(self, kind):
        """The image-to-image (`img2img`) or inpainting (`inpaint`) pipeline of the loaded model. A
        pipeline that takes a start image itself (InstructPix2Pix, Qwen-Image-Edit, FLUX.2 klein,
        FLUX.1 Kontext...) is its own image-to-image pipeline."""
        if kind == "img2img" and takes(self.pipe, "image"):
            return self.pipe
        if kind not in self.variants:
            import diffusers

            cls = {
                "img2img": diffusers.AutoPipelineForImage2Image,
                "inpaint": diffusers.AutoPipelineForInpainting,
            }[kind]
            try:
                self.variants[kind] = cls.from_pipe(self.pipe)
            except Exception as e:
                what = "image-to-image" if kind == "img2img" else "inpainting"
                raise BadRequest(f"this model has no {what} pipeline ({type(self.pipe).__name__}): {e}")
        return self.variants[kind]

    def device(self):
        import torch

        d = self.args.device
        if d == "auto":
            if torch.cuda.is_available():
                return "cuda"
            if getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
                return "mps"
            return "cpu"
        return d

    def dtype(self):
        import torch

        return {
            "auto": "auto",
            "float32": torch.float32,
            "float16": torch.float16,
            "bfloat16": torch.bfloat16,
        }[self.args.dtype]

    def load(self):
        try:
            import torch

            if self.args.threads > 0:
                torch.set_num_threads(self.args.threads)
            task, folder = self.args.task, self.args.model
            trust = False  # never: model code is not run
            refuse_custom_code(folder)
            if task in DIFFUSERS_TASKS:
                dtype = self.dtype()
                if dtype == "auto":
                    dtype = torch.float16 if self.device() == "cuda" else torch.float32
                self.pipe = load_text_to_image(folder, dtype, task).to(self.device())
                self.default_scheduler = self.pipe.scheduler
                low = self.args.low_memory
                if low == "on" or (low == "auto" and self.device() in ("cpu", "mps")):
                    # Attention in slices and the VAE a tile / an image at a time: a lower peak
                    # for a little time (the outputs stay the same up to the VAE's tile size).
                    if hasattr(self.pipe, "enable_attention_slicing"):
                        self.pipe.enable_attention_slicing()
                    vae = getattr(self.pipe, "vae", None)
                    if vae is not None and hasattr(vae, "enable_slicing"):
                        vae.enable_slicing()
                    if vae is not None and hasattr(vae, "enable_tiling"):
                        vae.enable_tiling()
            elif task == "text-ranking" or (
                task in ("feature-extraction", "sentence-similarity")
                and (
                    os.path.isfile(os.path.join(folder, "modules.json"))
                    or os.path.isfile(os.path.join(folder, "config_sentence_transformers.json"))
                )
            ):
                # Cross-encoders load without a sentence-transformers config too.
                import sentence_transformers as stm

                cls = stm.CrossEncoder if task == "text-ranking" else stm.SentenceTransformer
                self.st = cls(
                    folder,
                    device=self.device(),
                    trust_remote_code=trust,
                    local_files_only=True,
                )
            elif task == "automatic-speech-recognition" and is_ct2_whisper(folder):
                self.ct2 = load_ct2_whisper(folder, self.device(), self.args.threads)
            elif task in CHAT_TASKS:
                self.pipe = ChatRunner(task, folder, self.device(), self.dtype(), trust)
            elif task == "video-text-to-text":
                from transformers import AutoProcessor, AutoModelForImageTextToText

                self.processor = AutoProcessor.from_pretrained(folder, local_files_only=True, trust_remote_code=False)
                # Video frames already have bounded resolution. SmolVLM's photo defaults
                # upscale each frame to 2048 px and split it into tiles, overflowing its context.
                if getattr(self.processor, "image_processor", None) is not None:
                    image_processor = self.processor.image_processor
                    if type(image_processor).__name__.startswith("SmolVLM"):
                        image_processor.do_image_splitting = False
                        image_processor.size = {"longest_edge": 512}
                dtype = self.dtype()
                if dtype == "auto":
                    dtype = torch.float32 if self.device() == "cpu" else torch.float16
                self.pipe = AutoModelForImageTextToText.from_pretrained(
                    folder, local_files_only=True, trust_remote_code=False, dtype=dtype,
                ).to(self.device()).eval()
            elif task in NATIVE_TASKS:
                self.pipe = NativeRunner(task, folder, self.device(), self.dtype(), trust)
            else:
                from transformers import pipeline

                self.pipe = pipeline(
                    PIPELINE_NAME.get(task, task),
                    model=folder,
                    device=self.device(),
                    dtype=self.dtype(),
                    trust_remote_code=trust,
                )  # offline: HF_HUB_OFFLINE=1 in the environment
            self.state = "ok"
            self.loaded_at = int(time.time())
            log(f"ready: {task} {folder} on {self.device()}")
        except Exception as e:
            self.state = "error"
            self.error = f"{type(e).__name__}: {e}"
            log(traceback.format_exc())
            if self.exit_on_error:
                # Harness waits for a healthy /health: leave at once with the reason as the last
                # log line, so the launch fails now instead of at the load timeout.
                log(f"load failed: {self.error}")
                sys.stderr.flush()
                os._exit(3)


# --- faster-whisper (CTranslate2 Whisper exports) -------------------------------------------------

# Longest audio one request may carry (seconds of decoded audio).
MAX_AUDIO_SECONDS = 2 * 60 * 60
CT2_SAMPLE_RATE = 16000


def is_ct2_whisper(folder):
    """A CTranslate2 Whisper export (faster-whisper): `model.bin`, a vocabulary or tokenizer and a
    `config.json` without a transformers `model_type` / `architectures` (the CTranslate2 converter
    writes Whisper's `alignment_heads` / `lang_ids` / `suppress_ids` there). Original transformers
    weights next to it win."""
    try:
        names = set(os.listdir(folder))
        if "model.bin" not in names or "config.json" not in names:
            return False
        if any(n.endswith(".safetensors") for n in names):
            return False
        if not names & {"tokenizer.json", "vocabulary.json", "vocabulary.txt"}:
            return False
        with open(os.path.join(folder, "config.json"), encoding="utf-8") as f:
            cfg = json.load(f)
    except (OSError, ValueError):
        return False
    return (
        isinstance(cfg, dict)
        and "model_type" not in cfg
        and "architectures" not in cfg
        and any(k in cfg for k in ("alignment_heads", "lang_ids", "suppress_ids"))
    )


def load_ct2_whisper(folder, device, threads):
    """Loads the export with faster-whisper: int8 on the CPU, float16 on CUDA (int8_float16 then
    int8 where the GPU lacks it, the CPU when CUDA cannot start). The folder is a local path, so
    nothing is fetched."""
    import ctranslate2
    from faster_whisper import WhisperModel

    attempts = [("cpu", "int8")]
    if device == "cuda":
        attempts = [("cuda", "float16"), ("cuda", "int8_float16"), ("cuda", "int8")] + attempts
    last = None
    for dev, compute in attempts:
        try:
            if dev == "cuda" and ctranslate2.get_cuda_device_count() == 0:
                raise RuntimeError("CTranslate2 sees no CUDA device")
            m = WhisperModel(folder, device=dev, compute_type=compute, cpu_threads=max(threads, 0), local_files_only=True)
            log(f"faster-whisper on {dev} ({compute})")
            return m
        except Exception as e:
            last = e
            log(f"faster-whisper {dev}/{compute} failed: {type(e).__name__}: {e}")
    raise last


def language_name(code):
    """OpenAI's `verbose_json` names the language ("english"), faster-whisper gives its code."""
    try:
        from transformers.models.whisper.tokenization_whisper import LANGUAGES

        return LANGUAGES.get(code, code)
    except Exception:
        return code


def ct2_transcribe(model, data, form):
    """One OpenAI transcription / translation with faster-whisper: {text, language, duration,
    segments: [{id, start, end, text, words?}], words?}."""
    import numpy as np

    class Rate:  # to_audio resamples to the rate its `pipe.feature_extractor` names
        feature_extractor = type("F", (), {"sampling_rate": CT2_SAMPLE_RATE})()

    audio = to_audio(data, Rate)["raw"].astype(np.float32, copy=False)
    seconds = audio.size / CT2_SAMPLE_RATE
    if seconds > MAX_AUDIO_SECONDS:
        raise BadRequest(f"the audio is {int(seconds)} s long; the limit is {MAX_AUDIO_SECONDS} s")
    words_wanted = str(form.get("word_timestamps", "")).lower() in ("1", "true", "yes")
    kwargs = dict(
        task="translate" if form.get("task") == "translate" else "transcribe",
        beam_size=5,
        condition_on_previous_text=False,
        word_timestamps=words_wanted,
        vad_filter=seconds > 30,
    )
    if form.get("language"):
        kwargs["language"] = str(form.get("language")).strip().lower()
    if form.get("prompt"):
        kwargs["initial_prompt"] = form.get("prompt")
    if form.get("temperature") not in (None, ""):
        kwargs["temperature"] = float(form.get("temperature"))
    seg_iter, info = model.ct2.transcribe(audio, **kwargs)
    segments, words = [], []
    for i, seg in enumerate(seg_iter):
        item = {"id": i, "start": round(seg.start, 3), "end": round(seg.end, 3), "text": seg.text.strip()}
        if words_wanted:
            item["words"] = [
                {"word": w.word.strip(), "start": round(w.start, 3), "end": round(w.end, 3)} for w in seg.words or []
            ]
            words.extend(item["words"])
        segments.append(item)
    out = {
        "text": " ".join(s["text"] for s in segments if s["text"]).strip(),
        "language": language_name(info.language),
        "duration": round(seconds, 3),
        "segments": segments,
    }
    if words_wanted:
        out["words"] = words
    return out


class CustomCodeRefused(Exception):
    pass


class UnsafeWeights(Exception):
    pass


def pickle_weights(folder):
    """Whether a pipeline component's weights are a PyTorch `.bin` (a pickle) rather than
    safetensors: `unet/diffusion_pytorch_model.bin` (`segmind/tiny-sd` has only those)."""
    for entry in os.scandir(folder):
        if entry.is_dir():
            for name in os.listdir(entry.path):
                if name.endswith(".bin") and ".index" not in name:
                    return True
    return False


def torch_loads_weights_only(version):
    """torch >= 2.6 loads with `weights_only=True` by default: a pickle may hold tensors and
    plain containers, no callable (no code runs while it loads)."""
    major, minor = (int(x) for x in re.findall(r"\d+", version)[:2])
    return (major, minor) >= (2, 6)


def load_text_to_image(folder, dtype, task="text-to-image"):
    """A diffusers pipeline from its folder: safetensors only when no component has a `.bin`;
    `.bin` weights only through `torch.load(weights_only=True)` (diffusers and transformers pass
    it, and torch >= 2.6 defaults to it), never with an older torch. An `image-to-image` folder is
    loaded as the class `model_index.json` names, which must be one of diffusers' own."""
    import diffusers
    import torch

    pickles = pickle_weights(folder)
    if pickles and not torch_loads_weights_only(torch.__version__):
        raise UnsafeWeights(f"PyTorch .bin weights need torch 2.6 or later (found {torch.__version__})")
    if task == "image-to-image" or task in VIDEO_TASKS:
        with open(os.path.join(folder, "model_index.json"), encoding="utf-8") as f:
            name = json.load(f).get("_class_name")
        cls = getattr(diffusers, name, None) if isinstance(name, str) else None
        if not (isinstance(cls, type) and issubclass(cls, diffusers.DiffusionPipeline)):
            raise CustomCodeRefused(f"model_index.json names {name!r}, not a diffusers pipeline")
    else:
        cls = diffusers.AutoPipelineForText2Image
    return cls.from_pretrained(
        folder,
        torch_dtype=dtype,
        local_files_only=True,
        use_safetensors=None if pickles else True,
        trust_remote_code=False,
        variant=weights_variant(folder),
    )


def weights_variant(folder):
    """"fp16" when the pipeline's components hold only fp16 weights (the Hub download takes that
    one variant: `unet/diffusion_pytorch_model.fp16.safetensors`), else None (plain weights)."""
    fp16 = plain = False
    for entry in os.scandir(folder):
        if not entry.is_dir():
            continue
        for name in os.listdir(entry.path):
            stem, ext = os.path.splitext(name)
            if ext not in (".safetensors", ".bin") or ".index" in stem:
                continue
            stem = re.sub(r"-\d{5}-of-\d{5}$", "", stem)
            if stem.endswith(".fp16"):
                fp16 = True
            elif "." not in stem:
                plain = True
    return "fp16" if fp16 and not plain else None


# Libraries a diffusers `model_index.json` component may come from (anything else is a module of
# the repo itself).
INDEX_LIBRARIES = {"diffusers", "transformers", None}


def refuse_custom_code(folder):
    """Raises when the folder asks for code of its own: an `auto_map` in any config (the root,
    a component's, the tokenizer's), a diffusers component from another library, or a
    sentence-transformers module that is not one of the library's own."""

    def read(path):
        try:
            with open(path, encoding="utf-8") as f:
                return json.load(f)
        except (OSError, ValueError):
            return None

    configs = []
    for root, dirs, files in os.walk(folder):
        dirs[:] = [d for d in dirs if not d.startswith(".")]
        if os.path.relpath(root, folder).count(os.sep) > 1:
            continue
        for name in files:
            if name.endswith(".json"):
                configs.append(os.path.join(root, name))
    for path in configs:
        data = read(path)
        if isinstance(data, dict) and "auto_map" in data:
            raise CustomCodeRefused(f"{os.path.relpath(path, folder)} asks for remote code (auto_map)")
    index = read(os.path.join(folder, "model_index.json"))
    if isinstance(index, dict):
        for name, value in index.items():
            if isinstance(value, list) and value and value[0] not in INDEX_LIBRARIES:
                raise CustomCodeRefused(f"model_index.json component {name} comes from {value[0]}")
    modules = read(os.path.join(folder, "modules.json"))
    if isinstance(modules, list):
        for m in modules:
            kind = m.get("type", "") if isinstance(m, dict) else ""
            if not kind.startswith("sentence_transformers."):
                raise CustomCodeRefused(f"modules.json asks for a custom module ({kind})")


def log(msg):
    print(f"[hf-worker] {msg}", file=sys.stderr, flush=True)


# --- conversions -----------------------------------------------------------------------------


class BadRequest(Exception):
    pass


def jsonable(x):
    """numpy / torch values and PIL images as JSON (images as base64 PNG)."""
    try:
        import numpy as np
    except ImportError:  # pragma: no cover
        np = None
    if isinstance(x, dict):
        return {str(k): jsonable(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [jsonable(v) for v in x]
    if np is not None and isinstance(x, np.generic):
        return x.item()
    if np is not None and isinstance(x, np.ndarray):
        return x.tolist()
    if hasattr(x, "detach") and hasattr(x, "tolist"):
        return x.detach().cpu().tolist()
    if type(x).__module__.startswith("PIL."):
        return png_b64(x)
    return x


def png_b64(img):
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode()


def decode_bytes(value):
    """base64 or a data: URL to bytes."""
    if not isinstance(value, str):
        raise BadRequest("binary inputs are base64 strings or data: URLs")
    if value.startswith("data:"):
        value = value.split(",", 1)[-1]
    try:
        return base64.b64decode(value, validate=False)
    except Exception as e:
        raise BadRequest(f"invalid base64: {e}")


def to_image(value):
    from PIL import Image

    data = value if isinstance(value, (bytes, bytearray)) else decode_bytes(value)
    try:
        return Image.open(io.BytesIO(data)).convert("RGB")
    except Exception as e:
        raise BadRequest(f"not an image: {e}")


def to_audio(value, pipe=None):
    """bytes of any soundfile-readable format -> {"raw": float32 mono, "sampling_rate"}.

    With `pipe`, the audio is resampled here to the rate its feature extractor expects (16 kHz
    for Whisper / wav2vec2): transformers would otherwise ask for torchaudio, which the
    environment does not carry."""
    import numpy as np
    import soundfile as sf

    data = value if isinstance(value, (bytes, bytearray)) else decode_bytes(value)
    try:
        audio, sr = sf.read(io.BytesIO(data), dtype="float32", always_2d=True)
    except Exception as e:
        raise BadRequest(f"unreadable audio (wav, flac, ogg): {e}")
    raw = np.ascontiguousarray(audio.mean(axis=1))
    target = getattr(getattr(pipe, "feature_extractor", None), "sampling_rate", None)
    if isinstance(target, int) and target > 0 and sr != target and raw.size:
        from math import gcd
        from scipy.signal import resample_poly

        g = gcd(int(sr), target)
        raw = resample_poly(raw, target // g, int(sr) // g).astype("float32")
        sr = target
    return {"raw": raw, "sampling_rate": sr}


def wav_bytes(audio, rate):
    import numpy as np
    import soundfile as sf

    a = np.asarray(audio, dtype="float32").squeeze()
    if a.ndim > 1:  # (channels, samples) -> (samples, channels)
        a = a.T
    buf = io.BytesIO()
    sf.write(buf, a, int(rate), format="WAV", subtype="PCM_16")
    return buf.getvalue()


# --- tasks without a transformers 5 pipeline -----------------------------------------------------

# transformers 5 removed these pipelines (MIGRATION_GUIDE_V5: "use text-generation with a chat
# model"), but the models and their Auto classes remain; the Inference API shapes are kept here.
NATIVE_TASKS = {
    "question-answering",
    "summarization",
    "translation",
    "text2text-generation",
    "image-to-text",
    "visual-question-answering",
}


class NativeRunner:
    def __init__(self, task, folder, device, dtype, trust):
        import transformers as tf

        self.task, self.device = task, device
        kw = {"trust_remote_code": trust, "dtype": dtype}
        if task == "question-answering":
            self.tok = tf.AutoTokenizer.from_pretrained(folder, trust_remote_code=trust)
            self.model = tf.AutoModelForQuestionAnswering.from_pretrained(folder, **kw)
        elif task in ("summarization", "translation", "text2text-generation"):
            self.tok = tf.AutoTokenizer.from_pretrained(folder, trust_remote_code=trust)
            self.model = tf.AutoModelForSeq2SeqLM.from_pretrained(folder, **kw)
        elif task == "image-to-text":
            self.proc = tf.AutoProcessor.from_pretrained(folder, trust_remote_code=trust)
            self.model = tf.AutoModelForImageTextToText.from_pretrained(folder, **kw)
        else:  # visual-question-answering (ViLT-style classifiers)
            self.proc = tf.AutoProcessor.from_pretrained(folder, trust_remote_code=trust)
            self.model = tf.AutoModelForVisualQuestionAnswering.from_pretrained(folder, **kw)
        self.model.to(device).eval()

    def run(self, inputs, params):
        import torch

        with torch.inference_mode():
            return getattr(self, self.task.replace("-", "_"))(inputs, dict(params))

    def question_answering(self, inputs, params):
        import torch

        if not isinstance(inputs, dict) or "question" not in inputs or "context" not in inputs:
            raise BadRequest('"inputs" needs "question" and "context"')
        question, context = inputs["question"], inputs["context"]
        top_k = int(params.get("top_k") or 1)
        max_answer = int(params.get("max_answer_len") or 30)
        enc = self.tok(
            question, context, truncation="only_second", max_length=int(params.get("max_seq_len") or 384),
            stride=int(params.get("doc_stride") or 128), return_overflowing_tokens=True,
            return_offsets_mapping=True, padding=True, return_tensors="pt",
        )
        offsets = enc.pop("offset_mapping")
        enc.pop("overflow_to_sample_mapping", None)
        out = self.model(**{k: v.to(self.device) for k, v in enc.items()})
        candidates = []
        for w in range(out.start_logits.shape[0]):
            seq = enc.sequence_ids(w)
            start = torch.softmax(out.start_logits[w], -1).cpu()
            end = torch.softmax(out.end_logits[w], -1).cpu()
            ctx = [i for i, sid in enumerate(seq) if sid == 1]
            best_s = sorted(ctx, key=lambda i: -start[i])[:20]
            best_e = sorted(ctx, key=lambda i: -end[i])[:20]
            for a in best_s:
                for b in best_e:
                    if a <= b < a + max_answer:
                        cs, ce = int(offsets[w][a][0]), int(offsets[w][b][1])
                        candidates.append((float(start[a] * end[b]), cs, ce))
        seen, answers = set(), []
        for score, cs, ce in sorted(candidates, reverse=True):
            if (cs, ce) in seen:
                continue
            seen.add((cs, ce))
            answers.append({"answer": context[cs:ce], "score": score, "start": cs, "end": ce})
            if len(answers) == top_k:
                break
        if not answers:
            answers = [{"answer": "", "score": 0.0, "start": 0, "end": 0}]
        return answers[0] if top_k <= 1 else answers

    def _generate(self, texts, params, key):
        single = isinstance(texts, str)
        texts = [texts] if single else list(texts)
        gen = {k: params[k] for k in ("max_new_tokens", "num_beams", "do_sample", "temperature", "top_p") if k in params}
        gen.setdefault("max_new_tokens", 256)
        if self.task == "translation" and params.get("tgt_lang"):
            # NLLB / M2M style: the target language is the first generated token.
            lang_id = self.tok.convert_tokens_to_ids(params["tgt_lang"])
            if lang_id is not None and lang_id != self.tok.unk_token_id:
                gen["forced_bos_token_id"] = lang_id
            if params.get("src_lang") and hasattr(self.tok, "src_lang"):
                self.tok.src_lang = params["src_lang"]
        enc = self.tok(texts, return_tensors="pt", padding=True, truncation=True).to(self.device)
        ids = self.model.generate(**enc, **gen)
        out = [{key: t} for t in self.tok.batch_decode(ids, skip_special_tokens=True)]
        return out if single else [[o] for o in out]

    def summarization(self, inputs, params):
        return self._generate(inputs, params, "summary_text")

    def translation(self, inputs, params):
        return self._generate(inputs, params, "translation_text")

    def text2text_generation(self, inputs, params):
        return self._generate(inputs, params, "generated_text")

    def image_to_text(self, inputs, params):
        img = to_image(inputs)
        enc = self.proc(images=img, return_tensors="pt").to(self.device)
        ids = self.model.generate(**enc, max_new_tokens=int(params.get("max_new_tokens") or 64))
        return [{"generated_text": self.proc.batch_decode(ids, skip_special_tokens=True)[0].strip()}]

    def visual_question_answering(self, inputs, params):
        import torch

        if not isinstance(inputs, dict) or "image" not in inputs or "question" not in inputs:
            raise BadRequest('"inputs" needs "image" and "question"')
        enc = self.proc(to_image(inputs["image"]), inputs["question"], return_tensors="pt").to(self.device)
        probs = torch.sigmoid(self.model(**enc).logits[0]).cpu()
        top = torch.topk(probs, int(params.get("top_k") or 5))
        labels = self.model.config.id2label
        return [{"answer": labels[int(i)], "score": float(v)} for v, i in zip(top.values, top.indices)]


# --- task runners ----------------------------------------------------------------------------


def run_hf(model: Model, body: dict, raw: bytes | None):
    """POST /hf: the Hugging Face Inference API request and response shapes."""
    task = model.args.task
    if task in CHAT_TASKS:
        raise BadRequest("this model chats: use /v1/chat/completions")
    params = check_params(body.get("parameters") or {})
    inputs = body.get("inputs") if raw is None else raw
    if inputs is None:
        raise BadRequest('"inputs" is required')

    if task in DIFFUSERS_TASKS:
        if params.get("image") is not None:  # image-to-image / inpainting
            image, mask = params.pop("image"), params.pop("mask_image", None)
            img = edit_image(model, inputs, image, mask, params)[0]
        elif task == "image-to-image":
            raise BadRequest('this model starts from an image: "image" is required')
        else:
            img = text_to_image(model, inputs, params)[0]
        return {"image": png_b64(img)}
    if task in ("feature-extraction", "sentence-similarity") and model.st is not None:
        if task == "sentence-similarity":
            src, others = inputs["source_sentence"], inputs["sentences"]
            emb = model.st.encode([src] + list(others), normalize_embeddings=True)
            return [float(emb[0] @ e) for e in emb[1:]]
        return jsonable(model.st.encode(inputs))
    if task == "text-ranking" and model.st is not None:
        q, docs = inputs["query"], inputs["texts"]
        scores = model.st.predict([(q, d) for d in docs])
        return [{"index": i, "score": float(s)} for i, s in enumerate(scores)]

    pipe = model.pipe
    if task in NATIVE_TASKS:
        return jsonable(pipe.run(inputs, params))
    if task in IMAGE_TASKS:
        if task == "document-question-answering":
            q = inputs.get("question") if isinstance(inputs, dict) else params.pop("question", None)
            img = to_image(inputs["image"] if isinstance(inputs, dict) else inputs)
            return jsonable(pipe(image=img, question=q, **params))
        img = to_image(inputs)
        if task in ("zero-shot-image-classification",):
            return jsonable(pipe(img, candidate_labels=params.pop("candidate_labels"), **params))
        out = pipe(img, **params)
        # The shapes harness-onnx answers with too (one contract for both engines).
        if task == "depth-estimation":
            # `depth` is the pipeline's 8-bit image (value * 255 / max); min/max are the model's
            # own units, so a client can map the gray levels back.
            raw = out["predicted_depth"].squeeze()
            return {
                "depth": png_b64(out["depth"]),
                "width": out["depth"].size[0],
                "height": out["depth"].size[1],
                "min": float(raw.min()),
                "max": float(raw.max()),
            }
        if task == "image-segmentation":
            # Largest area first; semantic segmentation has no score (null).
            def area(o):
                return sum(1 for v in o["mask"].getdata() if v > 127)

            return [
                {"score": jsonable(o.get("score")), "label": o.get("label"), "mask": png_b64(o["mask"])}
                for o in sorted(out, key=area, reverse=True)
            ]
        if task == "object-detection":
            return jsonable(sorted(out, key=lambda o: -o["score"]))
        return jsonable(out)
    if model.ct2 is not None:
        out = ct2_transcribe(model, inputs if isinstance(inputs, (bytes, bytearray)) else decode_bytes(inputs), params)
        return {"text": out["text"]}
    if task in ("automatic-speech-recognition", "audio-classification", "zero-shot-audio-classification"):
        audio = to_audio(inputs, pipe)
        return jsonable(pipe(audio, **params))
    if task in ("text-to-speech", "text-to-audio"):
        out = pipe(inputs, **params)
        return {"audio": base64.b64encode(wav_bytes(out["audio"], out["sampling_rate"])).decode(),
                "sampling_rate": int(out["sampling_rate"])}
    if task == "zero-shot-classification":
        labels = params.pop("candidate_labels", None)
        if not labels:
            raise BadRequest('"parameters.candidate_labels" is required')
        return jsonable(pipe(inputs, candidate_labels=labels, **params))
    if task == "table-question-answering":
        return jsonable(pipe(table=inputs["table"], query=inputs["query"], **params))
    if task == "token-classification":
        params.setdefault("aggregation_strategy", "simple")
        return jsonable(pipe(inputs, **params))
    if task == "text-classification":
        # The Inference API answers a single text with a list of lists.
        out = pipe(inputs, top_k=params.pop("top_k", None), **params)
        if isinstance(inputs, str):
            return jsonable([out])
        return jsonable(out)
    return jsonable(pipe(inputs, **params))


# Samplers a diffusers request may pick (`sampler`), named like stable-diffusion.cpp's where they
# match: the diffusers scheduler class and its settings. `scheduler: karras` uses Karras sigmas
# with the samplers that have them; without `sampler` the folder's own scheduler runs.
SAMPLERS = {
    "euler": ("EulerDiscreteScheduler", {}),
    "euler_a": ("EulerAncestralDiscreteScheduler", {}),
    "heun": ("HeunDiscreteScheduler", {}),
    "dpm2": ("KDPM2DiscreteScheduler", {}),
    "dpm++2m": ("DPMSolverMultistepScheduler", {"algorithm_type": "dpmsolver++"}),
    "ddim": ("DDIMScheduler", {}),
    "pndm": ("PNDMScheduler", {}),
    "lms": ("LMSDiscreteScheduler", {}),
    "unipc": ("UniPCMultistepScheduler", {}),
    "lcm": ("LCMScheduler", {}),
}
SCHEDULES = ("default", "discrete", "karras")


def use_sampler(model: Model, pipe, sampler, schedule):
    """Puts the requested sampler (or the folder's own scheduler) on `pipe`."""
    base = model.default_scheduler
    if schedule not in (None, *SCHEDULES):
        raise BadRequest(f"scheduler {schedule!r} is not one of {', '.join(SCHEDULES)}")
    if sampler is None and schedule in (None, "default", "discrete"):
        pipe.scheduler = base
        return
    if sampler is not None and sampler not in SAMPLERS:
        raise BadRequest(f"sampler {sampler!r} is not one of {', '.join(SAMPLERS)}")
    import inspect

    import diffusers

    name, extra = SAMPLERS[sampler] if sampler else (type(base).__name__, {})
    cls = getattr(diffusers, name)
    extra = dict(extra)
    if schedule == "karras":
        if "use_karras_sigmas" not in inspect.signature(cls.__init__).parameters:
            raise BadRequest(f"the {sampler or name} sampler has no karras schedule")
        extra["use_karras_sigmas"] = True
    key = (name, tuple(sorted(extra.items())))
    if key not in model.schedulers:
        model.schedulers[key] = cls.from_config(base.config, **extra)
    pipe.scheduler = model.schedulers[key]


def diffusion_kwargs(model: Model, params, width=None, height=None):
    """The pipeline arguments of a request (OpenAI-route names and diffusers' own), with the size
    checked against the model's pixel limit."""
    import torch

    def pick(*names):
        for n in names:
            if params.get(n) is not None:
                return params[n]
        return None

    kw = {}
    for dst, names in (
        ("negative_prompt", ("negative_prompt",)),
        ("num_inference_steps", ("num_inference_steps", "steps")),
        ("guidance_scale", ("guidance_scale", "cfg_scale")),
        ("num_images_per_prompt", ("num_images_per_prompt", "n")),
    ):
        v = pick(*names)
        if v is not None:
            kw[dst] = v
    if "num_inference_steps" in kw:
        kw["num_inference_steps"] = int(kw["num_inference_steps"])
        if kw["num_inference_steps"] < 1:
            raise BadRequest("steps must be at least 1")
    width = pick("width") or width
    height = pick("height") or height
    for side, v in (("width", width), ("height", height)):
        if v is not None:
            if int(v) % 8 or int(v) < 64:
                raise BadRequest(f"{side} must be a multiple of 8, at least 64")
            kw[side] = int(v)
    if params.get("seed") is not None and int(params["seed"]) >= 0:
        kw["generator"] = torch.Generator(device="cpu").manual_seed(int(params["seed"]))
    # Without a size the pipeline uses the model's own (its sample size x the VAE's scale).
    unet = getattr(model.pipe, "unet", None) or getattr(model.pipe, "transformer", None)
    native = int(getattr(getattr(unet, "config", None), "sample_size", 64) or 64) * int(
        getattr(model.pipe, "vae_scale_factor", 8) or 8
    )
    pixels = kw.get("width", native) * kw.get("height", native) * int(kw.get("num_images_per_prompt", 1))
    if pixels > model.max_pixels():
        raise BadRequest(
            f"the request asks for {pixels / 1e6:.2f} megapixels (width x height x images); this "
            f"model allows {model.max_pixels() / 1e6:g} (the Largest image setting)"
        )
    return kw


def text_to_image(model: Model, prompt, params):
    kw = diffusion_kwargs(model, params)
    use_sampler(model, model.pipe, params.get("sampler"), params.get("scheduler"))
    return model.pipe(prompt=prompt, **kw).images


def to_mask(value):
    """An inpainting mask, white = repaint. OpenAI's masks (transparent = edit) are turned around;
    an opaque mask is read as grayscale."""
    from PIL import Image

    data = value if isinstance(value, (bytes, bytearray)) else decode_bytes(value)
    try:
        img = Image.open(io.BytesIO(data))
        img.load()
    except Exception as e:
        raise BadRequest(f"mask is not an image: {e}")
    if "A" in img.getbands():
        alpha = img.getchannel("A")
        if alpha.getextrema()[0] < 255:
            return alpha.point(lambda a: 255 if a < 128 else 0)
    return img.convert("L")


def edit_image(model: Model, prompt, image, mask, params):
    """Image-to-image (no mask) or inpainting (white = repaint) from `image`; without a size the
    image's own (rounded down to a multiple of 8) is used."""
    init = to_image(image[0] if isinstance(image, list) and image else image)
    w = int(params.get("width") or init.width - init.width % 8)
    h = int(params.get("height") or init.height - init.height % 8)
    kw = diffusion_kwargs(model, params, w, h)
    w, h = kw.pop("width"), kw.pop("height")
    if init.size != (w, h):
        init = init.resize((w, h))
    if params.get("strength") is not None:
        kw["strength"] = float(params["strength"])
    if mask is not None:
        if model.args.task == "image-to-image":
            raise BadRequest("this model edits a whole image: it takes no mask")
        pipe = model.variant("inpaint")
        kw.update(mask_image=to_mask(mask).resize((w, h)), width=w, height=h)
    else:
        pipe = model.variant("img2img")
        # The output size of a pipeline that takes it (FLUX.2 klein, Qwen-Image-Edit); others
        # follow the start image.
        kw.update(width=w, height=h)
    use_sampler(model, pipe, params.get("sampler"), params.get("scheduler"))
    # Only what this pipeline takes: InstructPix2Pix and the Edit pipelines have no `strength`.
    kw = {k: v for k, v in kw.items() if takes(pipe, k)}
    return pipe(prompt=prompt, image=init, **kw).images


def takes(pipe, name):
    """Whether the pipeline's `__call__` has the argument `name`."""
    import inspect

    return name in inspect.signature(type(pipe).__call__).parameters


# --- chat: text-generation and image-text-to-text ------------------------------------------------
#
# A transformers chat checkpoint (`AutoModelForCausalLM`, or `AutoModelForImageTextToText` with its
# processor for a vision model) behind OpenAI's /v1/chat/completions. The model's own chat template
# builds the prompt (tools included, when the template takes them), generation streams through a
# `TextIteratorStreamer`. The helpers below use the standard library only (the unit tests run them
# without torch).

DEFAULT_NEW_TOKENS = 2048
MAX_NEW_TOKENS = 32768
MAX_STOP_STRINGS = 4
MAX_IMAGES = 8
MAX_IMAGE_BYTES = 20 * 1024 * 1024
IMAGE_FETCH_SECONDS = 15
CHAT_ROLES = {"system", "developer", "user", "assistant", "tool"}


def _number(body, name, low, high, integer=False):
    value = body.get(name)
    if value is None:
        return None
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise BadRequest(f"{name!r} must be a number")
    if value < low or value > high:
        raise BadRequest(f"{name!r} must be between {low} and {high}")
    return int(value) if integer else float(value)


def chat_params(body):
    """The generation parameters of a chat request, checked. Limits are refused, not clamped."""
    if body.get("n") not in (None, 1):
        raise BadRequest('"n" must be 1: this engine returns one choice')
    stop = body.get("stop")
    if stop is None:
        stop = []
    elif isinstance(stop, str):
        stop = [stop]
    if not isinstance(stop, list) or not all(isinstance(s, str) and s for s in stop):
        raise BadRequest('"stop" must be a string or a list of non-empty strings')
    if len(stop) > MAX_STOP_STRINGS:
        raise BadRequest(f'"stop" takes at most {MAX_STOP_STRINGS} strings')
    limit = body.get("max_completion_tokens")
    if limit is None:
        limit = body.get("max_tokens")
    if limit is not None:
        limit = _number({"max_tokens": limit}, "max_tokens", 1, MAX_NEW_TOKENS, integer=True)
    options = body.get("stream_options")
    return {
        "max_new_tokens": limit,
        "temperature": _number(body, "temperature", 0, 5),
        "top_p": _number(body, "top_p", 0, 1),
        "top_k": _number(body, "top_k", 0, 100000, integer=True),
        "repetition_penalty": _number(body, "repetition_penalty", 0.01, 10),
        "seed": _number(body, "seed", -(2**63), 2**63 - 1, integer=True),
        "stop": stop,
        "stream": body.get("stream") is True,
        "include_usage": isinstance(options, dict) and options.get("include_usage") is True,
    }


def _fetch_image_bytes(url):
    """The bytes of an http(s) image URL: public addresses only (no loopback, private, link-local
    or reserved ones, so a request cannot reach services on this machine or its network), no
    redirects, no proxy, at most `MAX_IMAGE_BYTES`."""
    import socket
    import urllib.error
    import urllib.parse
    import urllib.request

    parts = urllib.parse.urlsplit(url)
    host = parts.hostname
    if parts.scheme not in ("http", "https") or not host:
        raise BadRequest("image_url must be a data: URL or an http(s) URL")
    try:
        addresses = {info[4][0] for info in socket.getaddrinfo(host, parts.port or 443)}
    except OSError as e:
        raise BadRequest(f"image_url: cannot resolve {host}: {e}")
    for address in addresses:
        ip = ipaddress.ip_address(address.split("%", 1)[0])
        if not ip.is_global or ip.is_multicast:
            raise BadRequest("image_url: only public addresses are fetched")

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *a, **k):
            return None

    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect)
    try:
        request = urllib.request.Request(url, headers={"User-Agent": "harness-studio"})
        with opener.open(request, timeout=IMAGE_FETCH_SECONDS) as r:
            data = r.read(MAX_IMAGE_BYTES + 1)
    except (urllib.error.URLError, OSError, ValueError) as e:
        raise BadRequest(f"image_url: could not fetch the image: {e}")
    if len(data) > MAX_IMAGE_BYTES:
        raise BadRequest(f"image_url: the image is larger than {MAX_IMAGE_BYTES} bytes")
    return data


def load_chat_image(url):
    """A PIL image from an `image_url` (`data:` URL or public http(s) URL)."""
    if not isinstance(url, str) or not url:
        raise BadRequest("image_url needs a url")
    if url.startswith("data:"):
        return to_image(url)
    return to_image(_fetch_image_bytes(url))


def _parts(content):
    """A message content as a list of ("text", str) / ("image", url)."""
    if content is None:
        return []
    if isinstance(content, str):
        return [("text", content)]
    if not isinstance(content, list):
        raise BadRequest("message content must be a string or a list of parts")
    out = []
    for part in content:
        if isinstance(part, str):
            out.append(("text", part))
            continue
        kind = part.get("type") if isinstance(part, dict) else None
        if kind in ("text", "input_text"):
            out.append(("text", str(part.get("text", ""))))
        elif kind in ("image_url", "input_image"):
            image = part.get("image_url", part.get("url"))
            url = image.get("url") if isinstance(image, dict) else image
            out.append(("image", url))
        else:
            raise BadRequest(f"content part type {kind!r} is not supported (text and image_url are)")
    return out


def _tool_arguments(value):
    """Tool call arguments as the dict chat templates render (OpenAI sends a JSON string)."""
    if isinstance(value, str):
        try:
            value = json.loads(value) if value.strip() else {}
        except ValueError:
            raise BadRequest("a tool call's arguments are not valid JSON")
    return value if isinstance(value, dict) else {}


def normalize_messages(messages, vision, load_image=load_chat_image):
    """OpenAI chat messages as `(template messages, images)`. A vision model gets every content as
    a list of `{"type": "text"|"image"}` parts and the PIL images in order; a text model gets
    strings (and refuses images)."""
    if not isinstance(messages, list) or not messages:
        raise BadRequest('"messages" must be a non-empty list')
    out, images = [], []
    for message in messages:
        if not isinstance(message, dict) or message.get("role") not in CHAT_ROLES:
            raise BadRequest("each message needs a role: system, user, assistant or tool")
        role = "system" if message["role"] == "developer" else message["role"]
        parts = _parts(message.get("content"))
        if any(kind == "image" for kind, _ in parts):
            if not vision:
                raise BadRequest("this model does not take images")
            if role != "user":
                raise BadRequest("images are only accepted in user messages")
        if vision and role != "tool":
            content = []
            for kind, value in parts:
                if kind == "image":
                    if len(images) >= MAX_IMAGES:
                        raise BadRequest(f"at most {MAX_IMAGES} images per request")
                    images.append(load_image(value))
                    content.append({"type": "image"})
                else:
                    content.append({"type": "text", "text": value})
        else:
            content = "".join(value for kind, value in parts if kind == "text")
        item = {"role": role, "content": content}
        calls = message.get("tool_calls")
        if role == "assistant" and isinstance(calls, list) and calls:
            item["tool_calls"] = [
                {
                    "id": c.get("id"),
                    "type": "function",
                    "function": {
                        "name": c["function"]["name"],
                        "arguments": _tool_arguments(c["function"].get("arguments")),
                    },
                }
                for c in calls
                if isinstance(c, dict) and isinstance(c.get("function"), dict) and c["function"].get("name")
            ]
        if role == "tool":
            for key in ("tool_call_id", "name"):
                if message.get(key):
                    item[key] = message[key]
        out.append(item)
    return out, images


def plain_prompt(messages):
    """A prompt for a model without a chat template (a base model): `role: text` lines."""
    lines = []
    for m in messages:
        content = m["content"]
        if isinstance(content, list):
            content = "".join(p.get("text", "") for p in content if p.get("type") == "text")
        lines.append(f"{m['role']}: {content}")
    return "\n".join(lines) + "\nassistant:"


_TOOL_BLOCK = re.compile(r"<tool_call>\s*(.*?)\s*(?:</tool_call>|\Z)", re.S)


def _as_call(obj, names):
    """One tool call object in any of the common shapes -> {"name", "arguments"}, else None."""
    if not isinstance(obj, dict):
        return None
    function = obj.get("function") if isinstance(obj.get("function"), dict) else obj
    name = function.get("name")
    if not isinstance(name, str) or (names and name not in names):
        return None
    arguments = function.get("arguments", function.get("parameters", {}))
    if not isinstance(arguments, str):
        arguments = json.dumps(arguments if arguments is not None else {}, ensure_ascii=False)
    return {"name": name, "arguments": arguments}


def _json_values(text):
    """The JSON values (objects and arrays) a text holds back to back, as (start, end, value)."""
    decoder = json.JSONDecoder()
    pos, out = 0, []
    while pos < len(text):
        if text[pos] not in "{[":
            pos += 1
            continue
        try:
            value, end = decoder.raw_decode(text, pos)
        except ValueError:
            pos += 1
            continue
        out.append((pos, end, value))
        pos = end
    return out


def parse_tool_calls(text, names=None):
    """`(content, calls)` for a model's answer: the tool calls it wrote in the shapes chat templates
    teach (`<tool_call>{json}</tool_call>` of Qwen and Hermes, `[TOOL_CALLS][...]` of Mistral,
    `<|python_tag|>{json}` of Llama 3.1, or an answer that is only the call as JSON) as OpenAI
    `tool_calls`, and the text left over. `names`: the offered tools; a call to another name is
    not a call. Without a call the text is returned as it is."""
    found, rest = [], text
    for block in _TOOL_BLOCK.finditer(text):
        values = _json_values(block.group(1))
        call = _as_call(values[0][2], names) if values else None
        if call:
            found.append(call)
            rest = rest.replace(block.group(0), "", 1)
    if not found:
        for marker in ("[TOOL_CALLS]", "<|python_tag|>"):
            if marker in text:
                head, _, tail = text.partition(marker)
                values = _json_values(tail)
                items = values[0][2] if values and isinstance(values[0][2], list) else [v for _, _, v in values]
                calls = [_as_call(v, names) for v in items]
                if calls and all(calls):
                    found, rest = calls, head
                break
    if not found:
        stripped = text.strip()
        values = _json_values(stripped)
        if values and values[0][0] == 0 and values[-1][1] == len(stripped):
            items = values[0][2] if isinstance(values[0][2], list) else [v for _, _, v in values]
            calls = [_as_call(v, names) for v in items]
            if calls and all(calls):
                found, rest = calls, ""
    if not found:
        return text, []
    tool_calls = [
        {
            "id": "call_" + uuid.uuid4().hex[:24],
            "type": "function",
            "function": {"name": c["name"], "arguments": c["arguments"]},
        }
        for c in found
    ]
    return rest.strip(), tool_calls


class StopTrimmer:
    """Cuts a stream of text at the first stop string. `feed` returns the text that is safe to
    send (a tail that could still become a stop string is held back); `done` is set once one was
    seen; `flush` releases what is held at the end of the answer."""

    def __init__(self, stops):
        self.stops = [s for s in stops if s]
        self.hold = max((len(s) for s in self.stops), default=1) - 1
        self.pending = ""
        self.done = False

    def feed(self, text):
        if self.done:
            return ""
        self.pending += text
        cut = min((i for i in (self.pending.find(s) for s in self.stops) if i >= 0), default=-1)
        if cut >= 0:
            out, self.pending, self.done = self.pending[:cut], "", True
            return out
        keep = min(self.hold, len(self.pending))
        out = self.pending[: len(self.pending) - keep]
        self.pending = self.pending[len(self.pending) - keep :]
        return out

    def flush(self):
        out, self.pending = ("" if self.done else self.pending), ""
        return out


def sse(payload):
    """One server-sent event; `payload` None is OpenAI's end marker."""
    if payload is None:
        return b"data: [DONE]\n\n"
    return b"data: " + json.dumps(payload, ensure_ascii=False).encode() + b"\n\n"


def chunk(completion_id, created, alias, delta, finish=None, usage=None):
    """A `chat.completion.chunk`; with `usage` it is the final usage-only chunk (no choices)."""
    out = {
        "id": completion_id,
        "object": "chat.completion.chunk",
        "created": created,
        "model": alias,
        "choices": [] if usage is not None else [{"index": 0, "delta": delta, "finish_reason": finish}],
    }
    if usage is not None:
        out["usage"] = usage
    return out


def completion(completion_id, created, alias, message, finish, usage):
    return {
        "id": completion_id,
        "object": "chat.completion",
        "created": created,
        "model": alias,
        "choices": [{"index": 0, "message": message, "finish_reason": finish}],
        "usage": usage,
    }


def usage_of(prompt_tokens, completion_tokens):
    return {
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": prompt_tokens + completion_tokens,
    }


def cpu_has_native_bf16():
    """Whether this CPU multiplies bfloat16 natively (AVX512-BF16 or AMX): anything else runs it
    far slower than float32."""
    try:
        import torch

        native = getattr(torch._C, "_cpu", None)
        return bool(native and (native._is_avx512_bf16_supported() or native._is_amx_tile_supported()))
    except Exception:
        return False


def config_dtype(folder):
    try:
        with open(os.path.join(folder, "config.json"), encoding="utf-8") as f:
            cfg = json.load(f)
    except (OSError, ValueError):
        return None
    return cfg.get("dtype") or cfg.get("torch_dtype") or (cfg.get("text_config") or {}).get("dtype")


class ChatRunner:
    """The chat model of this process: tokenizer or processor, and the generator."""

    def __init__(self, task, folder, device, dtype, trust):
        import torch
        import transformers as tf

        self.vision = task == "image-text-to-text"
        self.device = device
        if dtype == "auto":
            stored = config_dtype(folder)
            if device == "cuda":
                dtype = "auto"
            elif device == "mps":
                dtype = torch.float16
            elif stored in ("bfloat16", "float16") and cpu_has_native_bf16():
                dtype = torch.bfloat16
            else:
                dtype = torch.float32
        kw = {"trust_remote_code": trust, "dtype": dtype}
        if self.vision:
            self.proc = tf.AutoProcessor.from_pretrained(folder, trust_remote_code=trust)
            self.tok = getattr(self.proc, "tokenizer", None) or tf.AutoTokenizer.from_pretrained(folder, trust_remote_code=trust)
            self.model = tf.AutoModelForImageTextToText.from_pretrained(folder, **kw)
        else:
            self.proc = None
            self.tok = tf.AutoTokenizer.from_pretrained(folder, trust_remote_code=trust)
            self.model = tf.AutoModelForCausalLM.from_pretrained(folder, **kw)
        self.model.to(device).eval()
        template = getattr(self.proc if self.vision else self.tok, "chat_template", None)
        self.template = template if isinstance(template, str) else (json.dumps(template) if template else "")
        cfg = self.model.config
        text_cfg = cfg.get_text_config() if hasattr(cfg, "get_text_config") else cfg
        self.max_context = getattr(text_cfg, "max_position_embeddings", None) or getattr(cfg, "max_position_embeddings", None)

    def prepare(self, messages, images, tools, max_new_tokens):
        """Prompt tensors for the messages (tools only when the template mentions them), the
        prompt's length and the number of tokens that may be generated."""
        owner = self.proc if self.vision else self.tok
        kw = {"add_generation_prompt": True, "tokenize": False}
        if tools and "tools" in self.template:
            kw["tools"] = tools
        if self.template:
            try:
                text = owner.apply_chat_template(messages, **kw)
            except Exception as e:
                raise BadRequest(f"the chat template refused these messages: {e}")
        else:
            text = plain_prompt(messages)
        if self.vision:
            inputs = self.proc(text=text, images=images or None, return_tensors="pt")
        else:
            inputs = self.tok(text, return_tensors="pt", add_special_tokens=not self.template)
        tensors = {}
        for key, value in dict(inputs).items():
            if hasattr(value, "to"):
                if value.is_floating_point():
                    value = value.to(self.device, dtype=self.model.dtype)
                else:
                    value = value.to(self.device)
            tensors[key] = value
        prompt_tokens = int(tensors["input_ids"].shape[-1])
        room = (self.max_context - prompt_tokens) if self.max_context else MAX_NEW_TOKENS
        if room < 1:
            raise BadRequest(f"the prompt has {prompt_tokens} tokens; the model's context is {self.max_context}")
        return tensors, prompt_tokens, min(max_new_tokens or DEFAULT_NEW_TOKENS, room)

    def generate(self, tensors, prompt_tokens, max_new_tokens, p, streamer=None, cancel=None):
        """Runs generation; returns `(generated token ids, number of tokens generated)`."""
        import torch
        from transformers import StoppingCriteria, StoppingCriteriaList

        class Watch(StoppingCriteria):
            """Counts the generated tokens and stops when `cancel` is set (a client left)."""

            def __init__(self):
                self.generated = 0

            def __call__(self, input_ids, scores, **kwargs):
                self.generated = int(input_ids.shape[-1]) - prompt_tokens
                stop = bool(cancel and cancel.is_set())
                return torch.full((input_ids.shape[0],), stop, dtype=torch.bool, device=input_ids.device)

        watch = Watch()
        kw = dict(tensors, max_new_tokens=max_new_tokens, stopping_criteria=StoppingCriteriaList([watch]))
        eos = self.tok.eos_token_id
        pad = self.tok.pad_token_id
        kw["pad_token_id"] = pad if pad is not None else (eos[0] if isinstance(eos, list) else eos)
        temperature = p["temperature"]
        if temperature == 0:
            kw.update(do_sample=False, temperature=None, top_p=None, top_k=None)
        elif temperature is not None or p["top_p"] is not None or p["top_k"] is not None:
            kw["do_sample"] = True
            for key in ("temperature", "top_p", "top_k"):
                if p[key] is not None:
                    kw[key] = p[key]
        if p["repetition_penalty"] is not None:
            kw["repetition_penalty"] = p["repetition_penalty"]
        if p["stop"]:
            kw["stop_strings"] = p["stop"]
            kw["tokenizer"] = self.tok
        if streamer is not None:
            kw["streamer"] = streamer
        if p["seed"] is not None:
            torch.manual_seed(p["seed"])
        with torch.inference_mode():
            out = self.model.generate(**kw)
        return out[0][prompt_tokens:], watch.generated

    def decode(self, ids):
        return self.tok.decode(ids, skip_special_tokens=True)

    def ended(self, ids):
        """Whether generation ended on an end-of-sequence token (else it ran out of tokens)."""
        if len(ids) == 0:
            return True
        eos = self.model.generation_config.eos_token_id
        eos = eos if isinstance(eos, (list, tuple)) else [eos]
        return int(ids[-1]) in {int(e) for e in eos if e is not None}


def chat_request(model, body):
    """The checked pieces of a chat request: `(parameters, messages, images, tools, tool names)`."""
    p = chat_params(body)
    messages, images = normalize_messages(body.get("messages"), model.pipe.vision)
    tools = body.get("tools") if body.get("tool_choice") != "none" else None
    if tools is not None and (not isinstance(tools, list) or not all(isinstance(t, dict) for t in tools)):
        raise BadRequest('"tools" must be a list of tool definitions')
    names = {(t.get("function") or {}).get("name") for t in tools or []} - {None}
    return p, messages, images, tools or None, names


def run_chat(model, body):
    """A whole answer: the OpenAI chat completion."""
    runner = model.pipe
    p, messages, images, tools, names = chat_request(model, body)
    tensors, prompt_tokens, budget = runner.prepare(messages, images, tools, p["max_new_tokens"])
    ids, _ = runner.generate(tensors, prompt_tokens, budget, p)
    text = runner.decode(ids)
    finish = "stop" if runner.ended(ids) else "length"
    if p["stop"]:
        trimmer = StopTrimmer(p["stop"])
        text = trimmer.feed(text) + trimmer.flush()
        if trimmer.done:
            finish = "stop"
    message = {"role": "assistant", "content": text}
    if tools:
        content, calls = parse_tool_calls(text, names)
        if calls:
            message, finish = {"role": "assistant", "content": content or None, "tool_calls": calls}, "tool_calls"
    return completion(
        "chatcmpl-" + uuid.uuid4().hex[:24], int(time.time()), model.args.alias, message, finish,
        usage_of(prompt_tokens, int(len(ids))),
    )


def stream_chat(model, body):
    """The events of a streamed answer: a generator of SSE bytes that holds the model while it
    runs. With tools offered the answer is read to its end first (a call is only known then) and
    sent as one delta."""
    from transformers import TextIteratorStreamer

    runner = model.pipe
    p, messages, images, tools, names = chat_request(model, body)
    cid, created, alias = "chatcmpl-" + uuid.uuid4().hex[:24], int(time.time()), model.args.alias
    tensors, prompt_tokens, budget = runner.prepare(messages, images, tools, p["max_new_tokens"])

    def events():
        cancel = threading.Event()
        with model.lock:
            streamer = TextIteratorStreamer(runner.tok, skip_prompt=True, skip_special_tokens=True)
            result = {}

            def work():
                try:
                    result["ids"], result["generated"] = runner.generate(
                        tensors, prompt_tokens, budget, p, streamer, cancel
                    )
                except Exception as e:
                    log(traceback.format_exc())
                    result["error"] = f"{type(e).__name__}: {e}"
                    streamer.end()

            thread = threading.Thread(target=work, name="generate", daemon=True)
            thread.start()
            trimmer = StopTrimmer(p["stop"])
            full = []
            try:
                yield sse(chunk(cid, created, alias, {"role": "assistant", "content": ""}))
                for piece in streamer:
                    out = trimmer.feed(piece)
                    if out:
                        full.append(out)
                        if not tools:
                            yield sse(chunk(cid, created, alias, {"content": out}))
                    if trimmer.done:
                        cancel.set()
                        break
                tail = trimmer.flush()
                if tail:
                    full.append(tail)
                    if not tools:
                        yield sse(chunk(cid, created, alias, {"content": tail}))
                thread.join()
                if "error" in result:
                    yield sse({"error": {"message": result["error"], "type": "server_error", "code": None}})
                    yield sse(None)
                    return
                ids = result.get("ids", [])
                finish = "stop" if trimmer.done or runner.ended(ids) else "length"
                text = "".join(full)
                if tools:
                    content, calls = parse_tool_calls(text, names)
                    if calls:
                        indexed = [dict(c, index=i) for i, c in enumerate(calls)]
                        yield sse(chunk(cid, created, alias, {"tool_calls": indexed}))
                        finish = "tool_calls"
                    elif text:
                        yield sse(chunk(cid, created, alias, {"content": text}))
                yield sse(chunk(cid, created, alias, {}, finish))
                if p["include_usage"]:
                    yield sse(chunk(cid, created, alias, None, usage=usage_of(prompt_tokens, result.get("generated", 0))))
                yield sse(None)
            finally:
                cancel.set()
                thread.join(timeout=30)

    return events()


# --- HTTP --------------------------------------------------------------------------------------


class GenerationCancelled(Exception):
    pass


def video_frames(value, count=8):
    """Sample a bounded number of frames. Only inline media; never fetch client URLs or paths."""
    import av
    if len(value) > 45 * 1024 * 1024:
        raise BadRequest("video must be at most 32 MiB")
    raw = decode_bytes(value)
    if len(raw) > 32 * 1024 * 1024:
        raise BadRequest("video must be at most 32 MiB")
    with av.open(io.BytesIO(raw)) as container:
        stream = container.streams.video[0]
        duration = stream.duration
        frames = []
        if duration:
            for i in range(count):
                container.seek(int(duration * i / count), stream=stream)
                frame = next(container.decode(video=0), None)
                if frame is not None:
                    frames.append(frame.to_image().resize((384, 384)))
        else:
            for frame in container.decode(video=0):
                frames.append(frame.to_image().resize((384, 384)))
                if len(frames) == count:
                    break
    if not frames:
        raise BadRequest("video contains no decodable frames")
    return frames


def video_chat(model, body):
    import torch
    messages, images = [], []
    for message in body.get("messages", []):
        content = message.get("content", "")
        parts = [{"type": "text", "text": content}] if isinstance(content, str) else content
        converted = []
        for part in parts:
            kind = part.get("type")
            if kind == "text":
                converted.append({"type": "text", "text": part["text"]})
            elif kind in ("video_url", "input_video"):
                value = part.get("video_url", part.get("input_video"))
                if isinstance(value, dict):
                    value = value.get("url") or value.get("data")
                if not isinstance(value, str) or not value.startswith("data:video/"):
                    raise BadRequest("video content must be an inline data:video URL")
                sampled = video_frames(value)
                if len(images) + len(sampled) > 16:
                    raise BadRequest("at most two videos per request")
                images.extend(sampled)
                converted.extend({"type": "image"} for _ in sampled)
            else:
                raise BadRequest(f"unsupported video chat content part: {kind}")
        messages.append({"role": message["role"], "content": converted})
    if not images:
        raise BadRequest("include a video_url content part")
    prompt = model.processor.apply_chat_template(messages, add_generation_prompt=True)
    inputs = model.processor(text=prompt, images=images, return_tensors="pt").to(model.pipe.device)
    with torch.inference_mode():
        output = model.pipe.generate(**inputs, max_new_tokens=min(512, max(1, int(body.get("max_tokens", 128)))), do_sample=False)
    text = model.processor.batch_decode(output[:, inputs["input_ids"].shape[1]:], skip_special_tokens=True)[0]
    return {"id": f"chatcmpl-{time.time_ns()}", "object": "chat.completion", "created": int(time.time()),
            "model": model.args.alias, "choices": [{"index": 0, "message": {"role": "assistant", "content": text}, "finish_reason": "stop"}]}


def generate_video(model, body, job):
    import av
    import numpy as np
    fmt = body.get("output_format", "webm")
    if fmt not in ("mp4", "webm"):
        raise BadRequest("output_format must be mp4 or webm")
    pipe = model.pipe
    image = body.get("init_image")
    if image and not takes(pipe, "image"):
        import diffusers
        name = type(pipe).__name__.replace("Pipeline", "ImageToVideoPipeline")
        cls = getattr(diffusers, name, None)
        if cls is None:
            raise BadRequest("this model has no image-to-video pipeline")
        pipe = cls.from_pipe(pipe)
    if model.args.task == "image-to-video" and not image:
        raise BadRequest("this model requires an input_reference image")
    steps = int(body.get("sample_params", {}).get("sample_steps", 20))
    frames = int(body.get("video_frames", 9))
    width, height = int(body.get("width", 256)), int(body.get("height", 256))
    if not 1 <= frames <= 129 or not 1 <= steps <= 500 or width * height * frames > model.max_pixels() * 33:
        raise BadRequest("video exceeds the frame, step or pixel budget")
    params = {"steps": steps, "width": width, "height": height, "seed": body.get("seed", -1)}
    params.update({k: body[k] for k in ("negative_prompt",) if k in body})
    cfg = body.get("sample_params", {}).get("guidance", {}).get("txt_cfg")
    if cfg is not None:
        params["cfg_scale"] = cfg
    kw = diffusion_kwargs(model, params)
    kw["num_frames"] = frames
    if image:
        kw["image"] = to_image(image).resize((width, height))
    def step(_pipe, index, _timestep, kwargs):
        if job["cancel"].is_set():
            raise GenerationCancelled()
        job["preview"] = {"step": index + 1, "total_steps": steps}
        return kwargs
    kw["callback_on_step_end"] = step
    with model.lock:
        if job["cancel"].is_set():
            raise GenerationCancelled()
        result = pipe(prompt=body["prompt"], **kw).frames[0]
    if job["cancel"].is_set():
        raise GenerationCancelled()
    output = io.BytesIO()
    with av.open(output, mode="w", format=fmt) as container:
        stream = container.add_stream("libx264" if fmt == "mp4" else "libvpx-vp9", rate=int(body.get("fps", 16)))
        stream.width, stream.height, stream.pix_fmt = width, height, "yuv420p"
        for image in result:
            if job["cancel"].is_set():
                raise GenerationCancelled()
            arr = np.asarray(image)
            if arr.dtype != np.uint8:
                arr = (arr * 255).clip(0, 255).astype(np.uint8)
            frame = av.VideoFrame.from_ndarray(arr, format="rgb24")
            for packet in stream.encode(frame):
                container.mux(packet)
        for packet in stream.encode():
            container.mux(packet)
    return base64.b64encode(output.getvalue()).decode()


def build_app(model: Model):
    from starlette.applications import Starlette
    from starlette.concurrency import run_in_threadpool
    from starlette.requests import Request
    from starlette.responses import JSONResponse, Response, StreamingResponse
    from starlette.routing import Route

    def error(status, message, kind="invalid_request_error", code=None):
        return JSONResponse({"error": {"message": message, "type": kind, "code": code}}, status_code=status)

    def not_ready():
        if model.state == "loading":
            return error(503, "the model is loading", "unavailable", "model_loading")
        if model.state == "error":
            return error(500, model.error or "the model failed to load", "server_error", "model_failed")
        return None

    async def call(fn, *a):
        busy = not_ready()
        if busy is not None:
            return busy
        try:
            def locked():
                with model.lock:
                    return fn(*a)

            return await run_in_threadpool(locked)
        except BadRequest as e:
            return error(400, str(e))
        except (KeyError, TypeError, ValueError) as e:
            return error(400, f"{type(e).__name__}: {e}")
        except Exception as e:
            log(traceback.format_exc())
            return error(500, f"{type(e).__name__}: {e}", "server_error")

    def requires(*tasks):
        if model.args.task not in tasks:
            return error(404, f"this model serves {model.args.task}", "invalid_request_error", "wrong_task")
        return None

    async def health(_):
        if model.state == "ok":
            return JSONResponse({"status": "ok"})
        if model.state == "loading":
            return JSONResponse({"status": "loading"}, status_code=503)
        # The details (paths, the traceback) are in the server log, not on the wire.
        return JSONResponse({"status": "error", "error": "the model failed to load"}, status_code=500)

    async def models(_):
        return JSONResponse({
            "object": "list",
            "data": [{
                "id": model.args.alias,
                "object": "model",
                "owned_by": "harness",
                "created": model.loaded_at or 0,
                "task": model.args.task,
            }],
        })

    async def hf(request: Request):
        ctype = request.headers.get("content-type", "")
        if ctype.startswith("application/json") or not ctype:
            try:
                body = await request.json()
            except Exception:
                return error(400, "the body is not JSON")
            if not isinstance(body, dict):
                body = {"inputs": body}
            r = await call(run_hf, model, body, None)
        else:  # raw image / audio bytes, parameters (if any) in the query string as JSON
            raw = await request.body()
            params = request.query_params.get("parameters")
            body = {"parameters": json.loads(params) if params else {}}
            r = await call(run_hf, model, body, raw)
        return r if isinstance(r, Response) else JSONResponse(r)

    async def chat(request: Request):
        wrong = requires(*CHAT_TASKS, "video-text-to-text")
        if wrong:
            return wrong
        try:
            body = await request.json()
        except Exception:
            return error(400, "the body is not JSON")
        if not isinstance(body, dict):
            return error(400, "the body must be a JSON object")
        if model.args.task == "video-text-to-text":
            if body.get("stream"):
                return error(400, "video chat currently requires stream: false")
            r = await call(video_chat, model, body)
            return r if isinstance(r, Response) else JSONResponse(r)
        if body.get("stream") is True:
            r = await call(stream_chat, model, body)
            if isinstance(r, Response):
                return r
            return StreamingResponse(
                r, media_type="text/event-stream", headers={"Cache-Control": "no-cache", "X-Accel-Buffering": "no"}
            )
        r = await call(run_chat, model, body)
        return r if isinstance(r, Response) else JSONResponse(r)

    async def embeddings(request: Request):
        wrong = requires("feature-extraction", "sentence-similarity")
        if wrong:
            return wrong
        body = await request.json()
        texts = body.get("input")
        texts = [texts] if isinstance(texts, str) else texts
        if not texts:
            return error(400, '"input" is required')

        def run():
            if model.st is not None:
                vecs = model.st.encode(texts, normalize_embeddings=True)
                return [list(map(float, v)) for v in vecs]
            import numpy as np

            out = []
            for t in texts:  # token vectors -> mean
                arr = np.asarray(model.pipe(t)[0], dtype="float32")
                v = arr.mean(axis=0)
                out.append((v / (np.linalg.norm(v) or 1.0)).tolist())
            return out

        r = await call(run)
        if isinstance(r, Response):
            return r
        return JSONResponse({
            "object": "list",
            "model": model.args.alias,
            "data": [{"object": "embedding", "index": i, "embedding": v} for i, v in enumerate(r)],
            "usage": {"prompt_tokens": 0, "total_tokens": 0},
        })

    async def rerank(request: Request):
        wrong = requires("text-ranking")
        if wrong:
            return wrong
        body = await request.json()
        query, docs = body.get("query"), body.get("documents") or []
        docs = [d.get("text", "") if isinstance(d, dict) else str(d) for d in docs]
        if not query or not docs:
            return error(400, '"query" and "documents" are required')

        def run():
            if model.st is not None:
                return [float(s) for s in model.st.predict([(query, d) for d in docs])]
            out = model.pipe([{"text": query, "text_pair": d} for d in docs], top_k=1, function_to_apply="none")
            return [float(o[0]["score"] if isinstance(o, list) else o["score"]) for o in out]

        r = await call(run)
        if isinstance(r, Response):
            return r
        order = sorted(range(len(r)), key=lambda i: r[i], reverse=True)
        top_n = body.get("top_n") or len(order)
        results = []
        for i in order[:top_n]:
            item = {"index": i, "relevance_score": r[i]}
            if body.get("return_documents"):
                item["document"] = {"text": docs[i]}
            results.append(item)
        return JSONResponse({"model": model.args.alias, "results": results})

    async def transcriptions(request: Request):
        wrong = requires("automatic-speech-recognition")
        if wrong:
            return wrong
        form = await request.form()
        f = form.get("file")
        if f is None:
            return error(400, 'multipart field "file" is required')
        data = await f.read()
        if model.ct2 is not None:
            r = await call(ct2_transcribe, model, data, form)
            return r if isinstance(r, Response) else JSONResponse(r)
        gen = {}
        if form.get("language"):
            gen["language"] = form.get("language")
        if form.get("task") == "translate":
            # Whisper translates to English; other models have no such task (a 400 then).
            gen["task"] = "translate"
        params = {"generate_kwargs": gen} if gen else {}
        r = await call(lambda: model.pipe(to_audio(data, model.pipe), **params))
        if isinstance(r, Response):
            return r
        return JSONResponse({"text": r.get("text", "").strip()})

    async def speech(request: Request):
        wrong = requires("text-to-speech", "text-to-audio")
        if wrong:
            return wrong
        body = await request.json()
        text = body.get("input")
        if not text:
            return error(400, '"input" is required')
        r = await call(lambda: model.pipe(text))
        if isinstance(r, Response):
            return r
        return Response(wav_bytes(r["audio"], r["sampling_rate"]), media_type="audio/wav")

    # The fields both image routes read (OpenAI's plus Harness's extras, as the gateway sends them).
    IMAGE_FIELDS = (
        "negative_prompt", "num_inference_steps", "steps", "guidance_scale", "cfg_scale", "seed",
        "sampler", "scheduler",
    )

    def image_params(body):
        params = {k: body.get(k) for k in IMAGE_FIELDS}
        if isinstance(body.get("size"), str) and "x" in body["size"].lower():
            w, h = body["size"].lower().split("x", 1)
            try:
                params["width"], params["height"] = int(w), int(h)
            except ValueError:
                raise BadRequest('"size" must be WIDTHxHEIGHT')
        params["num_images_per_prompt"] = body.get("n") or 1
        return check_params({k: v for k, v in params.items() if v is not None})

    async def images(request: Request):
        wrong = requires("text-to-image")
        if wrong:
            return wrong
        body = await request.json()
        prompt = body.get("prompt")
        if not prompt:
            return error(400, '"prompt" is required')
        try:
            params = image_params(body)
        except BadRequest as e:
            return error(400, str(e))
        r = await call(text_to_image, model, prompt, params)
        if isinstance(r, Response):
            return r
        return JSONResponse({"created": int(time.time()), "data": [{"b64_json": png_b64(i)} for i in r]})

    async def edits(request: Request):
        wrong = requires("text-to-image", "image-to-image")
        if wrong:
            return wrong
        body = await request.json()
        prompt, image = body.get("prompt"), body.get("image")
        if not prompt:
            return error(400, '"prompt" is required')
        if not image:
            return error(400, '"image" is required')
        try:
            params = image_params(body)
            if body.get("strength") is not None:
                params.update(check_params({"strength": body["strength"]}))
        except BadRequest as e:
            return error(400, str(e))
        r = await call(edit_image, model, prompt, image, body.get("mask") or None, params)
        if isinstance(r, Response):
            return r
        return JSONResponse({"created": int(time.time()), "data": [{"b64_json": png_b64(i)} for i in r]})

    jobs = {}

    async def start_video(request: Request):
        wrong = requires(*VIDEO_TASKS) or not_ready()
        if wrong:
            return wrong
        if len(jobs) >= 8:
            return error(429, "too many video jobs")
        body = await request.json()
        if not isinstance(body.get("prompt"), str) or not body["prompt"].strip():
            return error(400, "prompt is required")
        job_id = str(time.time_ns())
        job = {"status": "generating", "cancel": threading.Event()}
        jobs[job_id] = job
        def run():
            try:
                result = generate_video(model, body, job)
                if not job["cancel"].is_set():
                    job.update(status="completed", result={"b64_json": result})
            except GenerationCancelled:
                job["status"] = "cancelled"
            except Exception as e:
                log(traceback.format_exc())
                job.update(status="failed", error={"message": str(e)})
        threading.Thread(target=run, daemon=True).start()
        return JSONResponse({"id": job_id})

    async def poll_video(request: Request):
        job = jobs.get(request.path_params["id"])
        if job is None:
            return error(404, "unknown video job")
        result = {k: v for k, v in job.items() if k != "cancel"}
        if job["status"] in ("completed", "failed", "cancelled"):
            jobs.pop(request.path_params["id"], None)
        return JSONResponse(result)

    async def cancel_video(request: Request):
        job = jobs.pop(request.path_params["id"], None)
        if job:
            job["cancel"].set()
        return JSONResponse({"status": "cancelled"})

    routes = [
        Route("/video/jobs", start_video, methods=["POST"]),
        Route("/video/jobs/{id}", poll_video),
        Route("/video/jobs/{id}/cancel", cancel_video, methods=["POST"]),
        Route("/health", health),
        Route("/v1/models", models),
        Route("/models", models),
        Route("/hf", hf, methods=["POST"]),
        Route("/v1/chat/completions", chat, methods=["POST"]),
        Route("/v1/embeddings", embeddings, methods=["POST"]),
        Route("/v1/rerank", rerank, methods=["POST"]),
        Route("/v1/audio/transcriptions", transcriptions, methods=["POST"]),
        Route("/v1/audio/speech", speech, methods=["POST"]),
        Route("/v1/images/generations", images, methods=["POST"]),
        Route("/v1/images/edits", edits, methods=["POST"]),
    ]
    app = Starlette(routes=routes)
    key = os.environ.get("HARNESS_WORKER_API_KEY", "")
    if not key:
        # Fail closed: without its key the worker would serve anyone on this machine.
        raise SystemExit("HARNESS_WORKER_API_KEY is not set; refusing to start")
    expected = f"Bearer {key}".encode()

    # Every route but /health needs the per-run key Harness passes in the environment (like
    # llama-server --api-key): the port is on loopback, but any local process could reach it.
    # Bodies are capped too (an image or a few minutes of audio fit easily).
    import hmac

    async def guarded(scope, receive, send):
        if scope["type"] == "http" and scope["path"] != "/health":
            headers = dict(scope.get("headers") or [])
            if not hmac.compare_digest(headers.get(b"authorization", b""), expected):
                await JSONResponse(
                    {"error": {"message": "invalid API key", "type": "authentication_error", "code": "invalid_api_key"}},
                    status_code=401,
                )(scope, receive, send)
                return
            try:
                declared = int(headers.get(b"content-length", b"0") or 0)
            except ValueError:
                declared = 0
            if declared > MAX_BODY_BYTES:
                await error(413, f"the request body is larger than {MAX_BODY_BYTES} bytes")(scope, receive, send)
                return
            seen = 0

            async def capped():
                nonlocal seen
                message = await receive()
                if message["type"] == "http.request":
                    seen += len(message.get("body", b""))
                    if seen > MAX_BODY_BYTES:
                        raise BadRequest(f"the request body is larger than {MAX_BODY_BYTES} bytes")
                return message

            await app(scope, capped, send)
            return
        await app(scope, receive, send)

    # Browsers: the preflight of an allowed origin is answered here (it carries no key), its
    # responses get CORS headers, and any other Origin is refused before the key is checked.
    origins = frozenset(o for o in os.environ.get("HARNESS_WORKER_ORIGINS", "").split(",") if o)
    preflight = [
        (b"access-control-allow-methods", b"GET, POST"),
        (b"access-control-allow-headers", b"authorization, content-type"),
        (b"access-control-max-age", b"600"),
    ]

    async def cors(scope, receive, send):
        if scope["type"] not in ("http", "websocket"):
            await guarded(scope, receive, send)
            return
        origin = dict(scope.get("headers") or []).get(b"origin")
        if origin is None:
            await guarded(scope, receive, send)
            return
        if origin.decode("latin-1") not in origins:
            if scope["type"] == "websocket":
                await send({"type": "websocket.close", "code": 1008})
            else:
                await error(403, "Origin not allowed.", "permission_error", "origin_not_allowed")(scope, receive, send)
            return
        if scope["type"] == "websocket":
            await guarded(scope, receive, send)
            return
        allow = [(b"access-control-allow-origin", origin), (b"vary", b"Origin")]
        if scope["method"] == "OPTIONS":
            await send({"type": "http.response.start", "status": 204, "headers": allow + preflight})
            await send({"type": "http.response.body", "body": b""})
            return

        async def with_cors(message):
            if message["type"] == "http.response.start":
                kept = [(k, v) for k, v in message.get("headers") or [] if not k.lower().startswith(b"access-control-") and k.lower() != b"vary"]
                message = dict(message, headers=kept + allow)
            await send(message)

        await guarded(scope, receive, with_cors)

    return cors


def main(argv=None):
    args = parse_args(argv)
    model = Model(args)
    model.exit_on_error = True
    threading.Thread(target=model.load, name="load", daemon=True).start()
    import uvicorn

    app = build_app(model)
    if args.uds:
        if os.path.exists(args.uds):
            os.unlink(args.uds)
        uvicorn.run(app, uds=args.uds, log_level="warning", access_log=False)
    else:
        uvicorn.run(app, host=args.host, port=args.port, log_level="warning", access_log=False)


if __name__ == "__main__":
    main()
