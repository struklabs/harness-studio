"""Harness MLX audio worker: text to speech or speech recognition with one local mlx-audio model folder.

    <env python> mlx_audio_worker.py --model <dir> --port <port> [--alias <name>]
        [--task speech|transcription] [--max-chars N] [--max-seconds N] [--max-audio-seconds N]

mlx-audio's own server is large (realtime websockets, transcription, voice lists fetched from
the Hub, reference audio read from a path the request names) and loads whatever a request
names. This worker serves only what Harness needs, itself:

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise); every route but
  /health needs "Authorization: Bearer <key>" (constant-time comparison of bytes). The key is
  removed from the environment once read and never logged.
* GET /health ({"status": "ok"}), GET /v1/models (the alias and the model's voices) and one
  POST route by --task: /v1/audio/speech ({"input", "voice", "speed", "language"} in, 16-bit
  WAV out) or /v1/audio/transcriptions (multipart "file" with optional "language" and "task"
  in, {"text"} out); every other route is 404.
* Browsers: only the origins in HARNESS_WORKER_ORIGINS (the Harness window and the user's CORS
  origins) get CORS headers; their preflight is answered here (it carries no key). A request
  with any other Origin is refused (403) before anything else, so no web page can drive it.
* The model is the folder given here, loaded at start. Voice cloning (a reference audio) is not
  offered and the audio to transcribe comes in the body (decoded in memory), so a request
  never names a file.
* It listens on 127.0.0.1 only; remote code is never trusted; Hugging Face Hub access is off;
  proxy variables are cleared.
* A request cannot run away: generation is capped at a number of codec frames that grows with
  the text (see `frame_budget`; sampling can miss the end token and keep talking), and it stops
  after --max-seconds of wall-clock time. Audio made by then is answered with
  "x-harness-truncated: time"; nothing made by then is a 504. A transcription takes a body of
  at most MAX_AUDIO_BODY bytes and audio of at most --max-audio-seconds, and stops with a 504
  after --max-seconds (checked between decoded windows / chunks).
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
SPEECH_PATH = "/v1/audio/speech"
TRANSCRIPTION_PATH = "/v1/audio/transcriptions"
FIELDS = {"model", "input", "voice", "speed", "language", "response_format"}
FORM_FIELDS = {"file", "model", "language", "task", "response_format"}
LABEL = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")
MAX_BODY = 1024 * 1024
# OpenAI's own upload limit for a transcription.
MAX_AUDIO_BODY = 25 * 1024 * 1024
# What the recognition models take: 16 kHz mono.
SAMPLE_RATE = 16000
DEFAULT_MAX_AUDIO_SECONDS = 1800
# Longer audio is fed to a chunking model (Parakeet) in windows of this many seconds.
CHUNK_SECONDS = 120.0
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
    p.add_argument("--task", choices=("speech", "transcription"), default="speech")
    p.add_argument("--max-audio-seconds", type=float, default=DEFAULT_MAX_AUDIO_SECONDS)
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


def form_fields(content_type, raw):
    """The fields of a multipart/form-data body as {name: bytes}; only FORM_FIELDS, each once."""
    kind, _, params = (content_type or "").partition(";")
    if kind.strip().lower() != "multipart/form-data":
        raise Refused("send the audio as multipart/form-data with a 'file' field")
    boundary = None
    for param in params.split(";"):
        name, _, value = param.strip().partition("=")
        if name.lower() == "boundary":
            boundary = value.strip().strip('"')
    if not boundary or len(boundary) > 200:
        raise Refused("the multipart body has no boundary")
    parts = raw.split(b"--" + boundary.encode("latin-1"))
    if len(parts) < 3 or parts[0].strip() or not parts[-1].startswith(b"--"):
        raise Refused("the multipart body is malformed")
    fields = {}
    for part in parts[1:-1]:
        if not part.startswith(b"\r\n") or not part.endswith(b"\r\n"):
            raise Refused("the multipart body is malformed")
        head, sep, body = part[2:-2].partition(b"\r\n\r\n")
        if not sep:
            raise Refused("the multipart body is malformed")
        name = None
        for line in head.decode("latin-1").split("\r\n"):
            key, _, value = line.partition(":")
            if key.strip().lower() == "content-disposition":
                found = re.search(r'(?:^|;)\s*name="([^"]*)"', value)
                name = found.group(1) if found else None
        if name is None:
            raise Refused("a multipart part has no name")
        if name not in FORM_FIELDS:
            raise Refused("unsupported field: %s" % name[:40])
        if name in fields:
            raise Refused("'%s' is given twice" % name)
        fields[name] = body
    return fields


def transcription_request(content_type, raw):
    """The checked fields of a transcription request: (audio bytes, language or None, task)."""
    fields = form_fields(content_type, raw)
    audio = fields.get("file")
    if not audio:
        raise Refused("the 'file' field (the audio to transcribe) is required")
    text = {}
    for name in ("language", "task", "response_format"):
        if name in fields:
            try:
                text[name] = fields[name].decode("utf-8").strip() or None
            except UnicodeDecodeError:
                raise Refused("'%s' must be text" % name)
    language = text.get("language")
    if language is not None and not LABEL.match(language):
        raise Refused("'language' must be a short name")
    task = text.get("task") or "transcribe"
    if task not in ("transcribe", "translate"):
        raise Refused("'task' must be transcribe or translate")
    if text.get("response_format") not in (None, "json"):
        raise Refused("the worker answers JSON only")
    return audio, language, task


def make_transcribe(model, max_seconds, max_audio_seconds, clock=time.monotonic):
    """`transcribe(samples, language, task)` -> text, for 16 kHz mono `samples`. Audio longer
    than `max_audio_seconds` is refused; a run is stopped with TooSlow after `max_seconds`:
    a Whisper model is checked before each 30 s window it decodes (its `decode`), a chunking
    model (Parakeet) after each chunk (`chunk_callback`)."""
    params = inspect.signature(model.generate).parameters
    translates = "task" in params
    chunked = "chunk_callback" in params
    state = {"deadline": None}

    def check(*_):
        if state["deadline"] is not None and clock() > state["deadline"]:
            raise TooSlow()

    decode = getattr(model, "decode", None)
    if not chunked and callable(decode):
        def checked_decode(*a, **kw):
            check()
            return decode(*a, **kw)

        model.decode = checked_decode

    def transcribe(samples, language, task):
        seconds = len(samples) / SAMPLE_RATE
        if seconds > max_audio_seconds:
            raise Refused("the audio is longer than %d seconds" % max_audio_seconds)
        kwargs = {}
        if translates:
            kwargs["task"] = task
            if language is not None:
                kwargs["language"] = language
        elif task == "translate":
            raise Refused("this model only transcribes")
        if chunked:
            kwargs["chunk_callback"] = check
            if seconds > CHUNK_SECONDS and "chunk_duration" in params:
                kwargs["chunk_duration"] = CHUNK_SECONDS
        state["deadline"] = clock() + max_seconds
        try:
            result = model.generate(samples, **kwargs)
        finally:
            state["deadline"] = None
        return heard_text(result, seconds)

    return transcribe


# A segment that starts this close to the end of the audio has no room for a word; one may end
# this much past it (timestamps are rounded to Whisper's 20 ms steps).
TAIL_SECONDS = 0.1
OVERRUN_SECONDS = 1.0


def heard_text(result, seconds):
    """The text of a recognition result. Whisper can keep decoding into the silence that pads its
    30 s window and make up segments there: ones that start or end past the end of the audio,
    end before they start or have no score (a NaN log probability) are dropped. Segments are
    kept as they come when any has no start time."""
    segments = getattr(result, "segments", None) or []
    timed = lambda s: isinstance(s, dict) and isinstance(s.get("text"), str) \
        and all(isinstance(s.get(k), (int, float)) for k in ("start", "end"))
    if segments and all(timed(s) for s in segments):
        def heard(s):
            score = s.get("avg_logprob", 0.0)
            return s["start"] < seconds - TAIL_SECONDS \
                and s["start"] <= s["end"] <= seconds + OVERRUN_SECONDS \
                and not (isinstance(score, float) and math.isnan(score))
        return "".join(s["text"] for s in segments if heard(s)).strip()
    text = getattr(result, "text", None)
    if not isinstance(text, str):
        raise RuntimeError("no text")
    return text.strip()


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


def app_with(path, handle, model_entry, max_body):
    """The ASGI app: /health, /v1/models (`model_entry`) and POST `path`, whose body (at most
    `max_body` bytes) goes to `await handle(body, content_type)` -> (status, payload,
    content_type, extra headers)."""
    expected = f"Bearer {KEY}".encode()
    routes = {("GET", "/health"), ("GET", "/v1/models"), ("POST", path)}

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
            return await respond(send, 200, {"object": "list", "data": [model_entry]})
        declared = headers.get(b"content-length", b"")
        if declared.isdigit() and int(declared) > max_body:
            return await respond(send, 413, error(
                "Request body too large.", "invalid_request_error", "body_too_large"))
        chunks, size, more = [], 0, True
        while more:
            message = await receive()
            if message["type"] == "http.disconnect":
                return
            chunk = message.get("body", b"")
            size += len(chunk)
            if size > max_body:
                return await respond(send, 413, error(
                    "Request body too large.", "invalid_request_error", "body_too_large"))
            chunks.append(chunk)
            more = message.get("more_body", False)
        content_type = headers.get(b"content-type", b"").decode("latin-1")
        status, payload, kind, extra = await handle(b"".join(chunks), content_type)
        await respond(send, status, payload, kind, extra)

    return app


def app_for(speak, alias, voices, max_chars):
    """The speech app; `speak(text, voice, speed, language)` returns (WAV bytes, truncated)
    (blocking)."""
    one_at_a_time = threading.Lock()

    def run(*args):
        with one_at_a_time:
            return speak(*args)

    async def handle(raw, _content_type):
        try:
            text, voice, speed, language = speech_request(raw, max_chars)
            if voice in OPENAI_VOICES and voice not in voices:
                voice = None
            if voice is not None and voices and voice not in voices:
                raise Refused("unknown voice %r; this model has: %s" % (voice, ", ".join(voices)))
        except Refused as e:
            return 400, error(str(e) + ".", "invalid_request_error", "invalid_request"), \
                b"application/json", []
        try:
            audio, truncated = await asyncio.to_thread(run, text, voice, speed, language)
        except TooSlow:
            return 504, error("Speech took longer than the worker's time limit.", "server_error",
                              "speech_timeout"), b"application/json", []
        except Exception as e:  # the model's own failure, without the folder's path
            return 500, error("Speech failed: %s" % type(e).__name__, "server_error",
                              "speech_failed"), b"application/json", []
        return 200, audio, b"audio/wav", [(b"x-harness-truncated", b"time")] if truncated else []

    entry = {"id": alias or "default_model", "object": "model", "owned_by": "harness",
             "voices": voices}
    return app_with(SPEECH_PATH, handle, entry, MAX_BODY)


def transcription_app_for(transcribe, decode, alias):
    """The speech recognition app; `decode(bytes)` makes 16 kHz mono samples of the uploaded
    audio and `transcribe(samples, language, task)` returns the text (both blocking)."""
    one_at_a_time = threading.Lock()

    def run(audio, language, task):
        try:
            samples = decode(audio)
        except Exception:
            raise Refused("the audio could not be decoded")
        with one_at_a_time:
            return transcribe(samples, language, task)

    def refused(e):
        return 400, error(str(e) + ".", "invalid_request_error", "invalid_request"), \
            b"application/json", []

    async def handle(raw, content_type):
        try:
            audio, language, task = transcription_request(content_type, raw)
        except Refused as e:
            return refused(e)
        try:
            text = await asyncio.to_thread(run, audio, language, task)
        except Refused as e:
            return refused(e)
        except TooSlow:
            return 504, error("Transcription took longer than the worker's time limit.",
                              "server_error", "transcription_timeout"), b"application/json", []
        except Exception as e:  # the model's own failure, without the folder's path
            return 500, error("Transcription failed: %s" % type(e).__name__, "server_error",
                              "transcription_failed"), b"application/json", []
        return 200, {"text": text}, b"application/json", []

    entry = {"id": alias or "default_model", "object": "model", "owned_by": "harness"}
    return app_with(TRANSCRIPTION_PATH, handle, entry, MAX_AUDIO_BODY)


def stt_options(model_dir):
    """Loader options of a recognition folder: a NeMo Parakeet config names no `model_type`
    (mlx-audio would guess it from the folder's name), so it is given here."""
    try:
        with open(os.path.join(model_dir, "config.json")) as f:
            config = json.load(f)
    except (OSError, ValueError):
        return {}
    if not config.get("model_type") and "EncDec" in str(config.get("target", "")):
        return {"model_type": "parakeet"}
    return {}


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

    if args.task == "transcription":
        import mlx.core as mx
        from mlx_audio.audio_io import read as read_audio
        from mlx_audio.stt.utils import load_model as load_stt

        model = load_stt(model_dir, **stt_options(model_dir))

        def decode(audio):
            samples, _ = read_audio(io.BytesIO(audio), dtype="float32", sample_rate=SAMPLE_RATE,
                                    nchannels=1)
            return mx.array(np.asarray(samples, dtype=np.float32).reshape(-1))

        transcribe = make_transcribe(model, args.max_seconds, args.max_audio_seconds)
        app = transcription_app_for(transcribe, decode, args.alias)
        uvicorn.run(cors(app), host=HOST, port=args.port, log_level="info", access_log=True)
        return

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
