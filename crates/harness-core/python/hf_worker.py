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
  POST /v1/embeddings              feature-extraction, sentence-similarity (OpenAI shape)
  POST /v1/rerank                  text-ranking (cross-encoders)
  POST /v1/audio/transcriptions    automatic-speech-recognition (multipart "file", OpenAI shape)
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
DIFFUSERS_TASKS = {"text-to-image"}
TASKS = TEXT_TASKS | IMAGE_TASKS | AUDIO_TASKS | DIFFUSERS_TASKS

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
        self.pipe = None
        self.st = None  # sentence-transformers model (embeddings / rerank), when the folder is one
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
        """The image-to-image (`img2img`) or inpainting (`inpaint`) pipeline of the loaded model."""
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
                self.pipe = load_text_to_image(folder, dtype).to(self.device())
                self.default_scheduler = self.pipe.scheduler
                low = self.args.low_memory
                if low == "on" or (low == "auto" and self.device() in ("cpu", "mps")):
                    # Attention in slices and the VAE a tile / an image at a time: a lower peak
                    # for a little time (the outputs stay the same up to the VAE's tile size).
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
        except Exception as e:  # reported by /health and every request
            self.state = "error"
            self.error = f"{type(e).__name__}: {e}"
            log(traceback.format_exc())


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


def load_text_to_image(folder, dtype):
    """A diffusers text-to-image pipeline from its folder: safetensors only when no component has
    a `.bin`; `.bin` weights only through `torch.load(weights_only=True)` (diffusers and
    transformers pass it, and torch >= 2.6 defaults to it), never with an older torch."""
    import torch
    from diffusers import AutoPipelineForText2Image

    pickles = pickle_weights(folder)
    if pickles and not torch_loads_weights_only(torch.__version__):
        raise UnsafeWeights(f"PyTorch .bin weights need torch 2.6 or later (found {torch.__version__})")
    return AutoPipelineForText2Image.from_pretrained(
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


def to_audio(value):
    """bytes of any soundfile-readable format -> {"raw": float32 mono, "sampling_rate"}."""
    import numpy as np
    import soundfile as sf

    data = value if isinstance(value, (bytes, bytearray)) else decode_bytes(value)
    try:
        audio, sr = sf.read(io.BytesIO(data), dtype="float32", always_2d=True)
    except Exception as e:
        raise BadRequest(f"unreadable audio (wav, flac, ogg): {e}")
    return {"raw": np.ascontiguousarray(audio.mean(axis=1)), "sampling_rate": sr}


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
    params = check_params(body.get("parameters") or {})
    inputs = body.get("inputs") if raw is None else raw
    if inputs is None:
        raise BadRequest('"inputs" is required')

    if task in DIFFUSERS_TASKS:
        if params.get("image") is not None:  # image-to-image / inpainting
            image, mask = params.pop("image"), params.pop("mask_image", None)
            img = edit_image(model, inputs, image, mask, params)[0]
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
    if task in ("automatic-speech-recognition", "audio-classification", "zero-shot-audio-classification"):
        audio = to_audio(inputs)
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
        pipe = model.variant("inpaint")
        kw.update(mask_image=to_mask(mask).resize((w, h)), width=w, height=h)
    else:
        pipe = model.variant("img2img")
    use_sampler(model, pipe, params.get("sampler"), params.get("scheduler"))
    return pipe(prompt=prompt, image=init, **kw).images


# --- HTTP --------------------------------------------------------------------------------------


def build_app(model: Model):
    from starlette.applications import Starlette
    from starlette.concurrency import run_in_threadpool
    from starlette.requests import Request
    from starlette.responses import JSONResponse, Response
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
        gen = {}
        if form.get("language"):
            gen["language"] = form.get("language")
        if form.get("task") == "translate":
            # Whisper translates to English; other models have no such task (a 400 then).
            gen["task"] = "translate"
        params = {"generate_kwargs": gen} if gen else {}
        r = await call(lambda: model.pipe(to_audio(data), **params))
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
        wrong = requires("text-to-image")
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

    routes = [
        Route("/health", health),
        Route("/v1/models", models),
        Route("/models", models),
        Route("/hf", hf, methods=["POST"]),
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
