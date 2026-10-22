"""Harness MLX vision worker: mlx-vlm's server for one local vision-language model folder.

    <env python> mlx_vlm_worker.py --model <dir> --port <port> [--alias <name>]
        [--max-tokens N] [--kv-bits 4|8]

mlx-vlm's server is large (images, audio, video, embeddings, image generation, management
routes) and loads whatever a request names. This worker puts a narrow door in front of it:

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise); every route but
  /health needs "Authorization: Bearer <key>" (constant-time comparison of bytes). The key is
  removed from the environment once read and never logged.
* Only GET /health, GET /v1/models and POST /v1/chat/completions are served; everything else
  (management, unload, image generation, audio, embeddings, websockets) is 404.
* Browsers: only the origins in HARNESS_WORKER_ORIGINS (the Harness window and the user's CORS
  origins) get CORS headers; their preflight is answered here (it carries no key). A request
  with any other Origin is refused (403) before anything else, so no web page can drive it.
  /health says only {"status": "ok"} (mlx-vlm's own lists the model path).
* The model is the folder given here, loaded at start: a request's "model" or adapter never
  loads another one.
* Images, audio and video must be inline: a base64 data: URL of their own kind
  (data:image/...;base64,), or base64 for input_audio, which is rewritten into a data: URL
  before mlx-vlm sees it. mlx-vlm would otherwise fetch an http URL or read a local file path a
  request names (and takes base64 that starts with "/" for a path).
* It listens on 127.0.0.1 only; remote code is never trusted; Hugging Face Hub access is off;
  proxy variables are cleared.
* Videos a model's processor cannot take natively become frames at the model's own video
  sampling (`video_sampling` in preprocessor_config.json: fps, at most --video-max-frames), and
  those frames skip image splitting the way the model's video processor would: a SmolVLM2 frame
  is one 512 px tile (64 tokens) instead of 13. The answer carries the frame count in
  `x-harness-video-frames`.
"""

import argparse
import base64
import binascii
import contextvars
import hmac
import json
import re
import os
import sys
import tempfile

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["MLX_TRUST_REMOTE_CODE"] = "false"
for _var in (
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy",
    # mlx-vlm's own key and model-folder discovery: Harness decides both.
    "MLX_VLM_SERVER_API_KEY", "MLX_VLM_MODEL_PATHS",
):
    os.environ.pop(_var, None)

KEY = os.environ.pop("HARNESS_WORKER_API_KEY", None) or None
ORIGINS = frozenset(o for o in os.environ.pop("HARNESS_WORKER_ORIGINS", "").split(",") if o)
HOST = "127.0.0.1"
ROUTES = {("GET", "/health"), ("GET", "/v1/models"), ("POST", "/v1/chat/completions")}
# Request fields that would load something other than the served folder.
REFUSED_FIELDS = ("adapter_path", "adapters", "draft_model", "model_path")
# Content parts whose source must be inline, by the data: URL kind each must carry.
MEDIA_KINDS = {
    "image_url": "image", "input_image": "image", "image": "image", "images": "image",
    "video_url": "video", "video": "video", "videos": "video",
    "audio": "audio", "audios": "audio",
}
# Keys under a media part that are labels, not sources.
LABEL_KEYS = ("detail", "format")
LABEL = re.compile(r"^[A-Za-z0-9_-]{1,16}$")
AUDIO_FORMATS = ("wav", "mp3", "flac", "ogg", "m4a", "aac", "webm", "opus", "pcm16")
MAX_BODY = 64 * 1024 * 1024
# Frames per request when the model names no smaller cap; mlx-vlm's own fallback budget.
VIDEO_MAX_FRAMES = 16
# mlx-vlm's fallback rate, used when the model has no video sampling of its own.
VIDEO_FPS = 2.0
# PIL `info` key on the frames of a video: the processor does not split them into tiles.
FRAME_MARK = "harness_video_frame"
# Frames sampled for the current request (a dict, so the request's own task can fill it).
FRAMES = contextvars.ContextVar("harness_video_frames", default=None)


def video_sampling(model_dir):
    """(fps, max_frames, longest_edge) of the model's video processor, each None if unset."""
    try:
        with open(os.path.join(model_dir, "preprocessor_config.json"), encoding="utf-8") as f:
            sampling = json.load(f).get("video_sampling") or {}
    except (OSError, ValueError, AttributeError):
        return None, None, None
    if not isinstance(sampling, dict):
        return None, None, None

    def positive(value, kind):
        return kind(value) if isinstance(value, (int, float)) and not isinstance(value, bool) \
            and value > 0 else None

    size = sampling.get("video_size")
    edge = positive(size.get("longest_edge"), int) if isinstance(size, dict) else None
    return positive(sampling.get("fps"), float), positive(sampling.get("max_frames"), int), edge


def frame_budget(model_dir, cap):
    """(fps, max_frames) for one request: the model's sampling, never above `cap` frames."""
    fps, max_frames, _ = video_sampling(model_dir)
    cap = max(2, int(cap))
    return fps or VIDEO_FPS, min(cap, max_frames) if max_frames else cap


def only_frames(images):
    """True when every image (nested lists allowed) is a sampled video frame."""
    flat = []
    stack = [images]
    while stack:
        item = stack.pop()
        if isinstance(item, (list, tuple)):
            stack.extend(item)
        elif item is not None:
            flat.append(item)
    return bool(flat) and all(getattr(i, "info", {}).get(FRAME_MARK) for i in flat)


def mark_frames(frames, edge):
    """Marks frames so the processor keeps each one whole, shrunk to `edge` px at most."""
    marked = []
    for frame in frames:
        if edge and max(frame.size) > edge:
            frame = frame.copy()
            frame.thumbnail((edge, edge))
        frame.info[FRAME_MARK] = True
        marked.append(frame)
    return marked


def keep_frames_whole(processor):
    """Image splitting off for calls that carry only video frames (the processor's class is
    swapped once for a subclass, so isinstance checks still hold)."""
    images = getattr(processor, "image_processor", None)
    if images is None or getattr(images, "_harness_frames", False):
        return
    base = type(images)

    def call(self, imgs, *args, **kwargs):
        if only_frames(imgs) and getattr(self, "do_image_splitting", False):
            kwargs["do_image_splitting"] = False
        return base.__call__(self, imgs, *args, **kwargs)

    images.__class__ = type(base.__name__, (base,), {"__call__": call, "_harness_frames": True})


VIDEO_SUFFIXES = {"video/mp4": ".mp4", "video/webm": ".webm", "video/quicktime": ".mov"}


def spill(video, folder, index):
    """A data: URL video written to a file in `folder` (OpenCV reads data: URLs through
    FFmpeg's data protocol, whose seeks fail past the first few frames); others unchanged."""
    if not isinstance(video, str) or not video.startswith("data:"):
        return video
    head, _, payload = video.partition(",")
    kind = head[5:].split(";")[0].lower()
    if not head.endswith(";base64") or kind not in VIDEO_SUFFIXES:
        return video
    path = os.path.join(folder, f"video{index}{VIDEO_SUFFIXES[kind]}")
    with open(path, "wb") as f:
        f.write(base64.b64decode(payload, validate=True))
    return path


def sample_videos(openai_module, video_module, model_dir, cap):
    """Routes mlx-vlm's video fallback through the model's sampling and records the count."""
    resolve = openai_module.resolve_video_inputs
    fps, max_frames = frame_budget(model_dir, cap)
    _, _, edge = video_sampling(model_dir)

    def resolve_capped(processor, videos, *, images=None, **_kwargs):
        if not videos or video_module.processor_handles_video(processor):
            return resolve(processor, videos, images=images, fps=fps, max_frames=max_frames)
        # The fallback decodes every frame here, so the files are not needed afterwards.
        with tempfile.TemporaryDirectory(prefix="harness-video-") as folder:
            files = [spill(v, folder, i) for i, v in enumerate(videos)]
            result = resolve(processor, files, images=images, fps=fps, max_frames=max_frames)
        if getattr(result, "used_fallback", False):
            keep = len(images or [])
            result.images[keep:] = mark_frames(result.images[keep:], edge)
            holder = FRAMES.get()
            if holder is not None:
                holder["frames"] = holder.get("frames", 0) + result.selected_count
        return result

    openai_module.resolve_video_inputs = resolve_capped


def parser():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--alias", default=None)
    p.add_argument("--max-tokens", type=int, default=8192)
    p.add_argument("--kv-bits", type=int, choices=[4, 8], default=None)
    p.add_argument("--video-max-frames", type=int, default=VIDEO_MAX_FRAMES)
    return p


class Refused(Exception):
    pass


def inline(source, kind):
    """`source` if it is a base64 data: URL of `kind` (image, audio, video)."""
    if not isinstance(source, str):
        raise Refused("a %s source must be a data: URL" % kind)
    head, comma, payload = source.strip().partition(",")
    if not (comma and head.startswith("data:%s/" % kind) and head.endswith(";base64")):
        raise Refused("%s sources must be inline data:%s/...;base64, URLs (got %r)"
                      % (kind.capitalize(), kind, source[:60]))
    try:
        base64.b64decode(payload, validate=True)
    except (binascii.Error, ValueError):
        raise Refused("the %s data: URL is not valid base64" % kind)
    return source


def media(value, kind):
    """A media part's value with every source checked (a URL string, a list of them or an
    object whose `url` is one; `detail` / `format` are labels)."""
    if isinstance(value, list):
        return [media(v, kind) for v in value]
    if isinstance(value, dict):
        out = {}
        for k, v in value.items():
            if k in LABEL_KEYS:
                if not (isinstance(v, str) and LABEL.match(v)):
                    raise Refused("bad %r" % k)
                out[k] = v
            else:
                out[k] = media(v, kind)
        return out
    return inline(value, kind)


def input_audio(value):
    """OpenAI's {"data": <base64>, "format": "wav"}, rewritten as {"data": "data:audio/wav;
    base64,..."} (mlx-vlm decodes that itself, and never takes it for a path or a URL)."""
    if not isinstance(value, dict):
        raise Refused("input_audio must be an object")
    fmt = value.get("format", "wav")
    if fmt not in AUDIO_FORMATS:
        raise Refused("unsupported input_audio format %r" % str(fmt)[:16])
    data = value.get("data")
    if isinstance(data, str) and data.strip().startswith("data:"):
        data = inline(data, "audio")
    else:
        if not isinstance(data, str):
            raise Refused("input_audio data must be base64")
        try:
            base64.b64decode(data.strip(), validate=True)
        except (binascii.Error, ValueError):
            raise Refused("input_audio data must be base64 (not a path or a URL)")
        data = "data:audio/%s;base64,%s" % (fmt, data.strip())
    out = dict(value)
    out["data"] = data
    out["format"] = fmt
    return out


def clean(value):
    """The messages with every media source checked (and input_audio rewritten)."""
    if isinstance(value, list):
        return [clean(v) for v in value]
    if not isinstance(value, dict):
        return value
    out = {}
    for k, v in value.items():
        if k == "file_id" or k == "file":
            raise Refused("files are not accepted; send media inline")
        if k == "input_audio":
            out[k] = input_audio(v)
        elif k in MEDIA_KINDS:
            out[k] = media(v, MEDIA_KINDS[k])
        else:
            out[k] = clean(v)
    return out


def check_body(raw):
    """(error message, None) for a chat body Harness won't pass on, else (None, the body to
    send on: the same bytes, or re-encoded when input_audio was rewritten)."""
    try:
        body = json.loads(raw or b"{}")
    except ValueError:
        return None, raw  # mlx-vlm answers malformed JSON itself
    if not isinstance(body, dict):
        return None, raw
    for field in REFUSED_FIELDS:
        if field in body:
            return f"'{field}' is not accepted: the served model is fixed.", None
    try:
        messages = clean(body.get("messages"))
    except Refused as e:
        return str(e)[:200] + ".", None
    if messages == body.get("messages"):
        return None, raw
    body["messages"] = messages
    return None, json.dumps(body).encode()


async def respond(send, status, payload):
    body = json.dumps(payload).encode()
    await send({
        "type": "http.response.start",
        "status": status,
        "headers": [(b"content-type", b"application/json"),
                    (b"content-length", str(len(body)).encode())],
    })
    await send({"type": "http.response.body", "body": body})


def error(message, kind, code):
    return {"error": {"message": message, "type": kind, "code": code}}


PREFLIGHT = [(b"access-control-allow-methods", b"GET, POST"),
             (b"access-control-allow-headers", b"authorization, content-type"),
             (b"access-control-max-age", b"600")]


def cors(inner):
    """Browser access for ORIGINS only: their preflight is answered here, their responses get
    CORS headers (any the inner app sets are dropped), any other Origin is refused with 403. A
    request without Origin (not a browser) passes unchanged."""

    async def app(scope, receive, send):
        if scope["type"] not in ("http", "websocket"):
            return await inner(scope, receive, send)
        origin = dict(scope.get("headers") or []).get(b"origin")
        if origin is None:
            return await inner(scope, receive, send)
        if origin.decode("latin-1") not in ORIGINS:
            if scope["type"] == "websocket":
                return await send({"type": "websocket.close", "code": 1008})
            return await respond(send, 403, error(
                "Origin not allowed.", "permission_error", "origin_not_allowed"))
        if scope["type"] == "websocket":
            return await inner(scope, receive, send)
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


def guard(inner, alias):
    expected = f"Bearer {KEY}".encode()

    async def app(scope, receive, send):
        if scope["type"] == "lifespan":
            return await inner(scope, receive, send)
        if scope["type"] != "http":
            if scope["type"] == "websocket":
                await send({"type": "websocket.close", "code": 1008})
            return
        method, path = scope["method"], scope["path"]
        if (method, path) not in ROUTES:
            return await respond(send, 404, error("Not found.", "not_found", "not_found"))
        if path == "/health":
            return await respond(send, 200, {"status": "ok"})
        auth = dict(scope.get("headers") or []).get(b"authorization", b"")
        if not hmac.compare_digest(auth, expected):
            return await respond(send, 401, error(
                "Missing or invalid API key.", "authentication_error", "invalid_api_key"))
        if path == "/v1/models":
            return await respond(send, 200, {"object": "list", "data": [
                {"id": alias or "default_model", "object": "model", "owned_by": "harness"}]})
        # Read the body to check it, then hand it on unchanged.
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
        problem, raw = check_body(b"".join(chunks))
        if problem:
            return await respond(send, 400, error(problem, "invalid_request_error", "invalid_request"))
        headers = [(k, v) for k, v in scope.get("headers") or [] if k != b"content-length"]
        headers.append((b"content-length", str(len(raw)).encode()))
        scope = dict(scope, headers=headers)
        sent = False

        async def replay():
            nonlocal sent
            if not sent:
                sent = True
                return {"type": "http.request", "body": raw, "more_body": False}
            return await receive()

        frames = {}
        FRAMES.set(frames)

        async def with_frames(message):
            if message["type"] == "http.response.start" and frames.get("frames"):
                message = dict(message, headers=list(message.get("headers") or []) + [
                    (b"x-harness-video-frames", str(frames["frames"]).encode())])
            await send(message)

        await inner(scope, replay, with_frames)

    return app


def main(argv=None):
    if KEY is None:
        sys.exit("mlx_vlm_worker: HARNESS_WORKER_API_KEY is required")
    args = parser().parse_args(argv)
    model = os.path.realpath(args.model)
    if not os.path.isdir(model):
        sys.exit("mlx_vlm_worker: the model must be a local folder")
    os.environ["MLX_VLM_PRELOAD_MODEL"] = model
    os.environ["MLX_VLM_MAX_TOKENS"] = str(args.max_tokens)
    if args.kv_bits:
        os.environ["KV_BITS"] = str(args.kv_bits)

    import uvicorn
    from fastapi import HTTPException

    import importlib

    import mlx_vlm.server as package

    # The package exports the FastAPI object as `app`; the module is what holds the loader.
    app_module = importlib.import_module("mlx_vlm.server.app")

    load = app_module.get_cached_model

    def pinned(*_args, model_kind="auto", **_kwargs):
        if model_kind not in ("auto", "text_generation"):
            raise HTTPException(status_code=404, detail="Not served here.")
        loaded = load(model, None, model_kind="text_generation")
        if isinstance(loaded, tuple) and len(loaded) > 1:
            keep_frames_whole(loaded[1])
        return loaded

    # Routes look it up on the package at call time; the start-up preload uses the module's.
    package.get_cached_model = pinned
    app_module.get_cached_model = pinned

    # Remote code is off whatever the environment says.
    generation = importlib.import_module("mlx_vlm.server.generation")
    load_weights = generation.load

    def load_no_remote_code(*a, **kw):
        kw["trust_remote_code"] = False
        return load_weights(*a, **kw)

    generation.load = load_no_remote_code

    sample_videos(importlib.import_module("mlx_vlm.server.openai"),
                  importlib.import_module("mlx_vlm.generate.video"), model, args.video_max_frames)

    uvicorn.run(cors(guard(app_module.app, args.alias)), host=HOST, port=args.port,
                log_level="info", access_log=True)


if __name__ == "__main__":
    main()
