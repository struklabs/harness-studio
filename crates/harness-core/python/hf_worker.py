"""Harness Studio python-hf worker: serves one Hugging Face model folder with a transformers
pipeline (or a diffusers text-to-image pipeline) over HTTP.

Started by Harness from its managed environment (`<data>/bin/pyenv/envs/hf-*`), one process per
model, on a loopback port or a Unix socket (Linux runs it without network, see
`pyenv::isolation`). The model is a local folder: nothing is downloaded, remote code is refused
unless `--trust-remote-code` is passed.

Endpoints
  GET  /health                     {"status":"ok"} | 503 {"status":"loading"} | 500 {"status":"error"}
  GET  /v1/models
  POST /hf                         Hugging Face Inference API: {"inputs": ..., "parameters": {...}}
  POST /v1/embeddings              feature-extraction, sentence-similarity (OpenAI shape)
  POST /v1/rerank                  text-ranking (cross-encoders)
  POST /v1/audio/transcriptions    automatic-speech-recognition (multipart "file", OpenAI shape)
  POST /v1/audio/speech            text-to-speech / text-to-audio ({"input": ...} -> audio/wav)
  POST /v1/images/generations      text-to-image (diffusers; OpenAI shape, b64_json)

Errors use the OpenAI shape {"error": {"message", "type", "code"}}.
"""

from __future__ import annotations

import argparse
import base64
import io
import ipaddress
import json
import os
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

# Tasks whose transformers pipeline name differs from the Hub tag.
PIPELINE_NAME = {
    "sentence-similarity": "feature-extraction",
    "text-ranking": "text-classification",
    "text-to-speech": "text-to-audio",
}


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--model", required=True, help="local model folder")
    p.add_argument("--task", required=True, choices=sorted(TASKS))
    p.add_argument("--alias", default=None)
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=0)
    p.add_argument("--uds", default=None, help="listen on this Unix socket instead of a port")
    p.add_argument("--device", default="auto", choices=["auto", "cpu", "cuda", "mps"])
    p.add_argument("--dtype", default="auto", choices=["auto", "float32", "float16", "bfloat16"])
    p.add_argument("--threads", type=int, default=0)
    p.add_argument("--trust-remote-code", action="store_true")
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
            trust = self.args.trust_remote_code
            if task in DIFFUSERS_TASKS:
                from diffusers import AutoPipelineForText2Image

                dtype = self.dtype()
                if dtype == "auto":
                    dtype = torch.float16 if self.device() == "cuda" else torch.float32
                self.pipe = AutoPipelineForText2Image.from_pretrained(
                    # safetensors when the repo has them, else .bin through torch.load(weights_only=True)
                    folder, torch_dtype=dtype, local_files_only=True, use_safetensors=None
                ).to(self.device())
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
    params = dict(body.get("parameters") or {})
    inputs = body.get("inputs") if raw is None else raw
    if inputs is None:
        raise BadRequest('"inputs" is required')

    if task in DIFFUSERS_TASKS:
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
        if task == "depth-estimation":
            return {"predicted_depth": jsonable(out["predicted_depth"]), "depth": png_b64(out["depth"])}
        if task == "image-segmentation":
            return [
                {"label": o.get("label"), "score": jsonable(o.get("score")), "mask": png_b64(o["mask"])}
                for o in out
            ]
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


def text_to_image(model: Model, prompt, params):
    import torch

    kw = {}
    for src, dst in (
        ("negative_prompt", "negative_prompt"),
        ("num_inference_steps", "num_inference_steps"),
        ("guidance_scale", "guidance_scale"),
        ("width", "width"),
        ("height", "height"),
        ("num_images_per_prompt", "num_images_per_prompt"),
    ):
        if params.get(src) is not None:
            kw[dst] = params[src]
    if params.get("seed") is not None:
        kw["generator"] = torch.Generator(device="cpu").manual_seed(int(params["seed"]))
    return model.pipe(prompt=prompt, **kw).images


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
        return JSONResponse({"status": "error", "error": model.error}, status_code=500)

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
        params = {}
        if form.get("language"):
            params["generate_kwargs"] = {"language": form.get("language")}
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

    async def images(request: Request):
        wrong = requires("text-to-image")
        if wrong:
            return wrong
        body = await request.json()
        prompt = body.get("prompt")
        if not prompt:
            return error(400, '"prompt" is required')
        params = {k: body.get(k) for k in ("negative_prompt", "num_inference_steps", "guidance_scale", "seed")}
        if isinstance(body.get("size"), str) and "x" in body["size"]:
            w, h = body["size"].lower().split("x", 1)
            params["width"], params["height"] = int(w), int(h)
        params["num_images_per_prompt"] = int(body.get("n") or 1)
        r = await call(text_to_image, model, prompt, params)
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
    ]
    return Starlette(routes=routes)


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
