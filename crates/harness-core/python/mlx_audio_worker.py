"""Harness MLX audio worker: text to speech with one local mlx-audio model folder.

    <env python> mlx_audio_worker.py --model <dir> --port <port> [--alias <name>]
        [--max-chars N] [--max-seconds N]

mlx-audio's own server is large (realtime websockets, transcription, voice lists fetched from
the Hub, reference audio read from a path the request names) and loads whatever a request
names. This worker serves only what Harness needs, itself:

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise); every route but
  /health needs "Authorization: Bearer <key>" (constant-time comparison of bytes). The key is
  removed from the environment once read and never logged.
* GET /health ({"status": "ok"}), GET /v1/models (the alias and the model's voices) and
  POST /v1/audio/speech ({"input", "voice", "speed", "language"} in, 16-bit WAV out); every
  other route is 404.
* Browsers: only the origins in HARNESS_WORKER_ORIGINS (the Harness window and the user's CORS
  origins) get CORS headers; their preflight is answered here (it carries no key). A request
  with any other Origin is refused (403) before anything else, so no web page can drive it.
* The model is the folder given here, loaded at start. Voice cloning (a reference audio) is not
  offered, so a request never names a file.
* It listens on 127.0.0.1 only; remote code is never trusted; Hugging Face Hub access is off;
  proxy variables are cleared.
* A request cannot run away: generation is capped at a number of codec frames that grows with
  the text (see `frame_budget`; sampling can miss the end token and keep talking), and it stops
  after --max-seconds of wall-clock time. Audio made by then is answered with
  "x-harness-truncated: time"; nothing made by then is a 504.
"""

import argparse
import asyncio
import hmac
import inspect
import io
import json
import math
import os
import re
import sys
import threading
import time
import wave

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
for _var in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
    os.environ.pop(_var, None)

KEY = os.environ.pop("HARNESS_WORKER_API_KEY", None) or None
ORIGINS = frozenset(o for o in os.environ.pop("HARNESS_WORKER_ORIGINS", "").split(",") if o)
HOST = "127.0.0.1"
ROUTES = {("GET", "/health"), ("GET", "/v1/models"), ("POST", "/v1/audio/speech")}
FIELDS = {"model", "input", "voice", "speed", "language", "response_format"}
LABEL = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")
MAX_BODY = 1024 * 1024
# OpenAI's voice names: a client written for OpenAI sends one; the model's default voice speaks.
OPENAI_VOICES = {"alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage",
                 "shimmer", "verse"}
# Codec frames per second of Qwen3-TTS (its "12Hz" tokenizer runs at 12.5 frames a second).
FRAME_RATE = 12.5
# Frames allowed per text token: speech takes about 6 (mlx-audio's own batch cap), so 8 leaves
# room for slow voices; never fewer than MIN_FRAMES (6 s), never more than MAX_AUDIO_SECONDS.
FRAMES_PER_TOKEN = 8
MIN_FRAMES = 75
MAX_AUDIO_SECONDS = 120
DEFAULT_MAX_SECONDS = 120


class TooSlow(Exception):
    """No audio was made within the request's time limit."""


def parser():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--alias", default=None)
    p.add_argument("--max-chars", type=int, default=4096)
    p.add_argument("--max-seconds", type=float, default=DEFAULT_MAX_SECONDS)
    return p


class Refused(Exception):
    pass


def speech_request(raw, max_chars):
    """The checked fields of a speech request: (text, voice or None, speed, language or None)."""
    try:
        body = json.loads(raw or b"{}")
    except ValueError:
        raise Refused("the body must be JSON")
    if not isinstance(body, dict):
        raise Refused("the body must be a JSON object")
    unknown = sorted(set(body) - FIELDS)
    if unknown:
        raise Refused("unsupported fields: %s" % ", ".join(unknown)[:120])
    text = body.get("input")
    if not isinstance(text, str) or not text.strip():
        raise Refused("'input' (the text to speak) is required")
    if len(text) > max_chars:
        raise Refused("'input' is longer than %d characters" % max_chars)
    labels = []
    for field in ("voice", "language"):
        value = body.get(field)
        if value is not None and not (isinstance(value, str) and LABEL.match(value)):
            raise Refused("'%s' must be a short name" % field)
        labels.append(value)
    speed = body.get("speed", 1.0)
    if isinstance(speed, bool) or not isinstance(speed, (int, float)) or not 0.25 <= speed <= 4.0:
        raise Refused("'speed' must be between 0.25 and 4")
    fmt = body.get("response_format", "wav")
    if fmt != "wav":
        raise Refused("the worker answers WAV only")
    return text, labels[0], float(speed), labels[1]


def wav_bytes(samples, rate):
    """16-bit mono PCM WAV of float samples in [-1, 1]."""
    import numpy as np

    pcm = (np.clip(np.asarray(samples, dtype=np.float32), -1.0, 1.0) * 32767.0).astype("<i2")
    out = io.BytesIO()
    with wave.open(out, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(int(rate))
        w.writeframes(pcm.tobytes())
    return out.getvalue()


def frame_budget(text_tokens, speed):
    """Codec frames one request may generate: grows with the text (and with slower speech),
    with a floor for very short inputs and a hard ceiling."""
    want = math.ceil(max(1, text_tokens) * FRAMES_PER_TOKEN / max(speed, 0.25))
    return min(max(MIN_FRAMES, want), int(MAX_AUDIO_SECONDS * FRAME_RATE))


def count_tokens(model, text):
    """Text tokens of `text` by the model's tokenizer; about one per two characters without one."""
    tokenizer = getattr(model, "tokenizer", None)
    try:
        return len(tokenizer.encode(text))
    except Exception:
        return len(text) // 2 + 1


def make_speak(model, voices, rate, max_seconds, encode, clock=time.monotonic):
    """`speak(text, voice, speed, language)` -> (WAV bytes, truncated). Generation is capped at
    `frame_budget` frames and stopped after `max_seconds` (between streamed chunks when the model
    can stream); `encode(chunks, rate)` makes the WAV of the audio chunks."""
    params = inspect.signature(model.generate).parameters
    takes = lambda name: name in params or any(
        p.kind is inspect.Parameter.VAR_KEYWORD for p in params.values())
    can_stream = "stream" in params

    def speak(text, voice, speed, language):
        kwargs = {"speed": speed}
        if voice is not None or voices:
            kwargs["voice"] = voice or voices[0]
        if language is not None:
            kwargs["lang_code"] = language
        if takes("max_tokens"):
            kwargs["max_tokens"] = frame_budget(count_tokens(model, text), speed)
        if can_stream:
            kwargs["stream"] = True
        deadline = clock() + max_seconds
        parts, truncated = [], False
        results = model.generate(text, **kwargs)
        try:
            for result in results:
                parts.append(result.audio)
                if clock() > deadline:
                    truncated = True
                    break
        finally:
            close = getattr(results, "close", None)
            if close:
                close()
        if not parts:
            if truncated or clock() > deadline:
                raise TooSlow()
            raise RuntimeError("no audio")
        return encode(parts, rate), truncated

    return speak


def voices_of(model):
    get = getattr(model, "get_supported_speakers", None)
    try:
        return [v for v in (get() if get else []) if isinstance(v, str)]
    except Exception:
        return []


async def respond(send, status, payload, content_type=b"application/json", extra=()):
    body = payload if isinstance(payload, bytes) else json.dumps(payload).encode()
    await send({
        "type": "http.response.start",
        "status": status,
        "headers": [(b"content-type", content_type),
                    (b"content-length", str(len(body)).encode())] + list(extra),
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


def app_for(speak, alias, voices, max_chars):
    """The ASGI app; `speak(text, voice, speed, language)` returns (WAV bytes, truncated)
    (blocking)."""
    expected = f"Bearer {KEY}".encode()
    one_at_a_time = threading.Lock()

    def run(*args):
        with one_at_a_time:
            return speak(*args)

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
                {"id": alias or "default_model", "object": "model", "owned_by": "harness",
                 "voices": voices}]})
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
        try:
            text, voice, speed, language = speech_request(b"".join(chunks), max_chars)
            if voice in OPENAI_VOICES and voice not in voices:
                voice = None
            if voice is not None and voices and voice not in voices:
                raise Refused("unknown voice %r; this model has: %s" % (voice, ", ".join(voices)))
        except Refused as e:
            return await respond(send, 400, error(
                str(e) + ".", "invalid_request_error", "invalid_request"))
        try:
            audio, truncated = await asyncio.to_thread(run, text, voice, speed, language)
        except TooSlow:
            return await respond(send, 504, error(
                "Speech took longer than the worker's time limit.", "server_error", "speech_timeout"))
        except Exception as e:  # the model's own failure, without the folder's path
            return await respond(send, 500, error(
                "Speech failed: %s" % type(e).__name__, "server_error", "speech_failed"))
        await respond(send, 200, audio, b"audio/wav",
                      [(b"x-harness-truncated", b"time")] if truncated else [])

    return app


def main(argv=None):
    if KEY is None:
        sys.exit("mlx_audio_worker: HARNESS_WORKER_API_KEY is required")
    args = parser().parse_args(argv)
    model_dir = os.path.realpath(args.model)
    if not os.path.isdir(model_dir):
        sys.exit("mlx_audio_worker: the model must be a local folder")

    import numpy as np
    import transformers
    import uvicorn

    # Remote code is off whatever a model's loader asks for.
    for name in ("AutoTokenizer", "AutoProcessor", "AutoConfig", "AutoFeatureExtractor"):
        cls = getattr(transformers, name, None)
        if cls is None:
            continue
        original = cls.from_pretrained

        def no_remote_code(*a, _original=original, **kw):
            kw["trust_remote_code"] = False
            return _original(*a, **kw)

        cls.from_pretrained = no_remote_code

    from mlx_audio.tts.utils import load_model

    model = load_model(model_dir)
    voices = voices_of(model)
    rate = getattr(model, "sample_rate", 24000)

    def encode(parts, rate):
        return wav_bytes(np.concatenate([np.asarray(p) for p in parts]), rate)

    speak = make_speak(model, voices, rate, args.max_seconds, encode)
    uvicorn.run(cors(app_for(speak, args.alias, voices, args.max_chars)), host=HOST, port=args.port,
                log_level="info", access_log=True)


if __name__ == "__main__":
    main()
