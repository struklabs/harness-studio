"""Harness MLX vision worker: mlx-vlm's server for one local vision-language model folder.

    <env python> mlx_vlm_worker.py --model <dir> --port <port> [--alias <name>]
        [--max-tokens N] [--kv-bits 4|8]

mlx-vlm's server is large (images, audio, video, embeddings, image generation, management
routes) and loads whatever a request names. This worker puts a narrow door in front of it:

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise); every route but
  /health needs "Authorization: Bearer <key>" (constant-time comparison of bytes). The key is
  removed from the environment once read and never logged.
* Only GET /health, GET /v1/models and POST /v1/chat/completions are served; everything else
  (management, unload, image generation, audio, embeddings, websockets, CORS preflight) is 404.
  /health says only {"status": "ok"} (mlx-vlm's own lists the model path).
* The model is the folder given here, loaded at start: a request's "model" or adapter never
  loads another one.
* Images, audio and video must be inline (data: URLs): mlx-vlm would otherwise fetch an http
  URL or read a local file path a request names.
* It listens on 127.0.0.1 only; remote code is never trusted; Hugging Face Hub access is off;
  proxy variables are cleared.
"""

import argparse
import hmac
import json
import os
import sys

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
HOST = "127.0.0.1"
ROUTES = {("GET", "/health"), ("GET", "/v1/models"), ("POST", "/v1/chat/completions")}
# Request fields that would load something other than the served folder.
REFUSED_FIELDS = ("adapter_path", "adapters", "draft_model", "model_path")
# Content parts whose source must be inline.
MEDIA_KEYS = ("image_url", "input_image", "image", "images", "video_url", "video", "videos",
              "input_audio", "audio", "audios", "file_id")
MAX_BODY = 64 * 1024 * 1024


def parser():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--alias", default=None)
    p.add_argument("--max-tokens", type=int, default=8192)
    p.add_argument("--kv-bits", type=int, choices=[4, 8], default=None)
    return p


def media_problem(value, inside_media=False):
    """The first media source in a request body that is not an inline data: URL, if any."""
    if isinstance(value, dict):
        for k, v in value.items():
            if inside_media and k == "data":
                continue  # input_audio's raw base64: inline
            found = media_problem(v, inside_media or k in MEDIA_KEYS)
            if found:
                return found
    elif isinstance(value, list):
        for v in value:
            found = media_problem(v, inside_media)
            if found:
                return found
    elif inside_media and isinstance(value, str) and not value.startswith("data:"):
        # Formats and detail levels are not sources.
        if value in ("auto", "low", "high", "wav", "mp3", "flac", "ogg", "png", "jpeg"):
            return None
        return value[:60]
    return None


def check_body(raw):
    """An error message for a chat body Harness won't pass on, or None."""
    try:
        body = json.loads(raw or b"{}")
    except ValueError:
        return None  # mlx-vlm answers malformed JSON itself
    if not isinstance(body, dict):
        return None
    for field in REFUSED_FIELDS:
        if field in body:
            return f"'{field}' is not accepted: the served model is fixed."
    bad = media_problem(body.get("messages"))
    if bad:
        return "Images, audio and video must be inline data: URLs (got %r)." % bad
    return None


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
        raw = b"".join(chunks)
        problem = check_body(raw)
        if problem:
            return await respond(send, 400, error(problem, "invalid_request_error", "invalid_request"))
        sent = False

        async def replay():
            nonlocal sent
            if not sent:
                sent = True
                return {"type": "http.request", "body": raw, "more_body": False}
            return await receive()

        await inner(scope, replay, send)

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
        return load(model, None, model_kind="text_generation")

    # Routes look it up on the package at call time; the start-up preload uses the module's.
    package.get_cached_model = pinned
    app_module.get_cached_model = pinned

    uvicorn.run(guard(app_module.app, args.alias), host=HOST, port=args.port,
                log_level="info", access_log=True)


if __name__ == "__main__":
    main()
