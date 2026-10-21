"""Harness Studio Liquid Audio worker: serves one LFM2.5-Audio model (speech output) over HTTP
with Liquid's official `liquid-audio` package.

Started by Harness from its managed environment (`<data>/bin/pyenv/envs/liquid-audio`), one
process per model, on a loopback port or a Unix socket (Linux runs it without network, see
`pyenv::isolation`). The model is a local folder (the Hub repo `LiquidAI/LFM2.5-Audio-1.5B`):
nothing is downloaded.

Endpoints
  GET  /health            {"status":"ok"} | 503 {"status":"loading"} | 500 {"status":"error"}
  GET  /v1/models
  POST /v1/chat/completions  OpenAI chat: text and `input_audio` parts in; text out, and speech out
                          too when the request says `"modalities": ["text", "audio"]`
                          (message.audio = {id, data (base64 WAV), transcript}). `stream` is answered
                          as a short SSE stream of the finished answer.
  POST /v1/audio/speech   JSON {"input", "voice", "response_format" (wav | pcm), "speed" (ignored)}.
                          Answers the audio (24 kHz mono 16-bit). Headers: x-harness-audio-seconds,
                          x-harness-generation-seconds (so a caller can compute the real-time factor).

`voice` is one of us_male, us_female, uk_male, uk_female (OpenAI's names map onto them).
Errors use the OpenAI shape {"error": {"message", "type", "code"}}. Every route but /health needs
"Authorization: Bearer <HARNESS_WORKER_API_KEY>"; the worker refuses to start without that key.
Browsers: only the origins in HARNESS_WORKER_ORIGINS get CORS headers and a preflight answer; any
other Origin is refused with 403.

One run at a time: the model computes one request at a time, so concurrent requests wait.
"""

from __future__ import annotations

import argparse
import base64
import hmac
import io
import json
import os
import re
import socketserver
import sys
import threading
import time
import uuid
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# The model is a local folder: nothing may reach the Hub (or telemetry) from this process.
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")

SAMPLE_RATE = 24000
MAX_BODY_BYTES = 32 * 1024 * 1024
#: Longest text answer (tokens) and the default one of a chat turn.
CHAT_MAX_TOKENS = 1024
CHAT_DEFAULT_TOKENS = 512
MAX_CLIPS = 4
MAX_CLIP_SECONDS = 120
CHAT_SYSTEM_SPEECH = "Respond with interleaved text and audio."
CHAT_SYSTEM_TEXT = "Respond with text."
#: Characters of one generation: longer text is split at sentence ends and the parts joined.
CHUNK_CHARS = 280
#: Audio tokens (12.5 a second) a part may use at most.
MAX_NEW_TOKENS = 600
VOICES = {
    "us_male": "Perform TTS. Use the US male voice.",
    "us_female": "Perform TTS. Use the US female voice.",
    "uk_male": "Perform TTS. Use the UK male voice.",
    "uk_female": "Perform TTS. Use the UK female voice.",
}
DEFAULT_VOICE = "us_female"
#: OpenAI's voice names, answered with a voice of the model.
OPENAI_VOICES = {
    "alloy": "us_female",
    "ash": "us_male",
    "ballad": "uk_male",
    "coral": "us_female",
    "echo": "us_male",
    "fable": "uk_male",
    "nova": "us_female",
    "onyx": "us_male",
    "sage": "uk_female",
    "shimmer": "uk_female",
    "verse": "uk_male",
}


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True, help="folder of LFM2.5-Audio")
    p.add_argument("--alias", default=None)
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=0)
    p.add_argument("--uds", default=None, help="listen on this Unix socket instead of a port")
    p.add_argument("--device", default="auto", choices=("auto", "cpu", "cuda", "mps"))
    args = p.parse_args(argv)
    if args.uds is None and not args.port:
        p.error("--port or --uds is required")
    return args


class Failure(Exception):
    """An answer other than 200: status, message, OpenAI error type and code."""

    def __init__(self, status, message, kind="invalid_request_error", code=None):
        super().__init__(message)
        self.status = status
        self.kind = kind
        self.code = code


def voice_key(voice) -> str:
    """The model voice a request's `voice` names."""
    if voice is None or voice == "":
        return DEFAULT_VOICE
    if isinstance(voice, dict):
        voice = voice.get("id")
    name = str(voice).strip().lower().replace("-", "_").replace(" ", "_")
    if name in VOICES:
        return name
    if name in OPENAI_VOICES:
        return OPENAI_VOICES[name]
    raise Failure(
        400,
        f"`{voice}` is not a voice of this model. Voices: {', '.join(VOICES)}.",
        code="voice_not_found",
    )


def split_text(text: str, limit: int = CHUNK_CHARS) -> list:
    """The text in parts of at most `limit` characters, cut after sentence ends (or spaces)."""
    sentences = re.split(r"(?<=[.!?。！？])\s+", text.strip())
    parts, current = [], ""
    for sentence in sentences:
        while len(sentence) > limit:
            cut = sentence.rfind(" ", 0, limit)
            cut = cut if cut > 0 else limit
            head, sentence = sentence[:cut].strip(), sentence[cut:].strip()
            if current:
                parts.append(current)
                current = ""
            parts.append(head)
        if current and len(current) + 1 + len(sentence) > limit:
            parts.append(current)
            current = sentence
        else:
            current = f"{current} {sentence}".strip()
    if current:
        parts.append(current)
    return [p for p in parts if p]


def wav_bytes(pcm16: bytes) -> bytes:
    out = io.BytesIO()
    with wave.open(out, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(pcm16)
    return out.getvalue()

def message_text(content, role):
    """`(text, clips)` of one message's content: a string, or text and `input_audio` parts."""
    if content is None:
        return "", []
    if isinstance(content, str):
        return content, []
    if not isinstance(content, list):
        raise Failure(400, "message `content` must be a string or a list of parts", code="bad_content")
    texts, clips = [], []
    for part in content:
        kind = part.get("type") if isinstance(part, dict) else None
        if kind in ("text", "input_text"):
            texts.append(str(part.get("text") or ""))
        elif kind == "input_audio":
            if role != "user":
                raise Failure(400, "audio is only accepted in user messages", code="bad_content")
            audio = part.get("input_audio")
            if not isinstance(audio, dict) or not isinstance(audio.get("data"), str) or not audio["data"]:
                raise Failure(400, "input_audio needs base64 data", code="bad_content")
            fmt = str(audio.get("format") or "wav").lower()
            if fmt not in ("wav", "mp3", "flac", "ogg"):
                raise Failure(400, f"input_audio format `{fmt}` is not supported (wav, mp3, flac, ogg)", code="bad_content")
            clips.append(audio["data"])
        else:
            raise Failure(400, f"content part type `{kind}` is not supported (text and input_audio are)", code="bad_content")
    return "\n".join(t for t in texts if t), clips


def chat_plan(body):
    """The checked pieces of a chat request: system text, the turns `[(role, text, [base64 clips])]`,
    whether speech is asked, the token budget, the text temperature and whether it streams."""
    messages = body.get("messages")
    if not isinstance(messages, list) or not messages:
        raise Failure(400, "`messages` must be a non-empty list", code="bad_messages")
    modalities = body.get("modalities")
    speech = False
    if modalities is not None:
        if not isinstance(modalities, list) or not all(m in ("text", "audio") for m in modalities):
            raise Failure(400, '`modalities` must be a list of "text" and "audio"', code="bad_modalities")
        speech = "audio" in modalities
    options = body.get("audio")
    if speech and options is not None:
        if not isinstance(options, dict):
            raise Failure(400, "`audio` must be an object", code="bad_audio")
        fmt = str(options.get("format") or "wav").lower()
        if fmt != "wav":
            raise Failure(400, f"audio format `{fmt}` is not served (wav)", code="format_unavailable")
    system, turns, clips = [], [], 0
    for m in messages:
        if not isinstance(m, dict):
            raise Failure(400, "every message must be an object", code="bad_messages")
        role = m.get("role")
        if role == "developer":
            role = "system"
        if role not in ("system", "user", "assistant"):
            raise Failure(400, f"message role `{role}` is not supported (system, user, assistant)", code="bad_messages")
        text, found = message_text(m.get("content"), role)
        clips += len(found)
        if clips > MAX_CLIPS:
            raise Failure(400, f"at most {MAX_CLIPS} audio clips per request", code="too_many_clips")
        if role == "system":
            if text:
                system.append(text)
        else:
            turns.append((role, text, found))
    if not turns or turns[-1][0] != "user":
        raise Failure(400, "the last message must come from the user", code="bad_messages")
    budget = body.get("max_completion_tokens", body.get("max_tokens"))
    if budget is None:
        budget = CHAT_DEFAULT_TOKENS
    if isinstance(budget, bool) or not isinstance(budget, int) or budget < 1:
        raise Failure(400, "`max_tokens` must be a positive integer", code="bad_max_tokens")
    temperature = body.get("temperature")
    if temperature is not None and (
        isinstance(temperature, bool) or not isinstance(temperature, (int, float)) or temperature < 0
    ):
        raise Failure(400, "`temperature` must be a number of 0 or more", code="bad_temperature")
    return {
        "system": "\n".join(system) or (CHAT_SYSTEM_SPEECH if speech else CHAT_SYSTEM_TEXT),
        "turns": turns,
        "speech": speech,
        "max_tokens": min(budget, CHAT_MAX_TOKENS),
        "temperature": temperature or None,
        "stream": body.get("stream") is True,
    }


def audio_message(wav, transcript):
    """OpenAI's `message.audio`: the answer's speech and what it says."""
    return {
        "id": "audio_" + uuid.uuid4().hex[:24],
        "data": base64.b64encode(wav).decode(),
        "transcript": transcript,
        "expires_at": int(time.time()) + 3600,
    }


def completion(alias, text, wav, prompt_tokens, completion_tokens, finish):
    """The OpenAI chat completion of an answer (`wav`: its speech, `None` for text only)."""
    message = {"role": "assistant", "content": text}
    if wav:
        message["audio"] = audio_message(wav, text)
    return {
        "id": "chatcmpl-" + uuid.uuid4().hex[:24],
        "object": "chat.completion",
        "created": int(time.time()),
        "model": alias,
        "choices": [{"index": 0, "message": message, "finish_reason": finish}],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens,
        },
    }


def completion_events(done):
    """The SSE events (bytes) that stream a finished completion: role, content, audio, finish."""
    choice = done["choices"][0]
    message = choice["message"]
    base = {"id": done["id"], "object": "chat.completion.chunk", "created": done["created"], "model": done["model"]}

    def event(delta, finish=None, **extra):
        data = dict(base, choices=[{"index": 0, "delta": delta, "finish_reason": finish}], **extra)
        return b"data: " + json.dumps(data).encode() + b"\n\n"

    out = [event({"role": "assistant"})]
    if message["content"]:
        out.append(event({"content": message["content"]}))
    if "audio" in message:
        out.append(event({"audio": message["audio"]}))
    out.append(event({}, choice["finish_reason"], usage=done["usage"]))
    out.append(b"data: [DONE]\n\n")
    return out


class Model:
    def __init__(self, args):
        self.args = args
        self.name = args.alias or os.path.basename(os.path.normpath(args.model))
        self.state = "loading"
        self.error = None
        self.lock = threading.Lock()
        self.torch = None
        self.processor = None
        self.model = None
        self.device = "cpu"

    def load(self):
        try:
            import torch

            device = self.args.device
            if device == "auto":
                if torch.cuda.is_available():
                    device = "cuda"
                elif getattr(torch.backends, "mps", None) and torch.backends.mps.is_available():
                    device = "mps"
                else:
                    device = "cpu"
            # bfloat16 is fast on CUDA; the CPU and MPS kernels are faster and safer in float32.
            dtype = torch.bfloat16 if device == "cuda" else torch.float32
            from liquid_audio import LFM2AudioDetokenizer, LFM2AudioModel, LFM2AudioProcessor
            from pathlib import Path
            from safetensors.torch import load_file
            from transformers import Lfm2Config

            folder = Path(self.args.model)
            processor = LFM2AudioProcessor.from_pretrained(folder, device=device).eval()
            model = LFM2AudioModel.from_pretrained(folder, device=device, dtype=dtype).eval()
            # liquid-audio builds the audio detokenizer on `.cuda()`; build it on the chosen device.
            detok_dir = folder / "audio_detokenizer"
            cfg = Lfm2Config.from_pretrained(detok_dir / "config.json")
            cfg.layer_types = [
                "full_attention" if t == "sliding_attention" else t for t in cfg.layer_types
            ]
            detok = LFM2AudioDetokenizer(cfg).eval()
            detok.load_state_dict(load_file(detok_dir / "model.safetensors"))
            processor._audio_detokenizer = detok.to(device)
            self.torch, self.processor, self.model = torch, processor, model
            self.device, self.dtype = device, dtype
            self.state = "ok"
            print(
                f"liquid-audio: {self.name} on {device} ({str(dtype).replace('torch.', '')})",
                file=sys.stderr,
                flush=True,
            )
        except Exception as e:  # the load error is what /health and the log say
            self.error = str(e)
            self.state = "error"
            print(f"load failed: {e}", file=sys.stderr, flush=True)

    def synthesize_part(self, text: str, voice: str):
        """The 24 kHz samples (a float tensor) of one part of text."""
        from liquid_audio import ChatState

        torch = self.torch
        chat = ChatState(self.processor, dtype=self.dtype)
        chat.new_turn("system")
        chat.add_text(VOICES[voice])
        chat.end_turn()
        chat.new_turn("user")
        chat.add_text(text)
        chat.end_turn()
        chat.new_turn("assistant")
        codes = []
        with torch.no_grad():
            for t in self.model.generate_sequential(
                **chat, max_new_tokens=MAX_NEW_TOKENS, audio_temperature=0.8, audio_top_k=64
            ):
                if t.numel() > 1:
                    codes.append(t)
            if len(codes) < 2:
                raise Failure(500, "the model produced no audio", "server_error", "no_audio")
            # The last frame is the end-of-audio code.
            audio_codes = torch.stack(codes[:-1], 1).unsqueeze(0)
            return self.processor.decode(audio_codes).float().cpu()[0]

    def load_clip(self, data: str):
        """`(samples, rate)` of a base64 audio clip: a float tensor of shape (1, n)."""
        import soundfile

        try:
            samples, rate = soundfile.read(io.BytesIO(base64.b64decode(data)), dtype="float32", always_2d=True)
        except Exception:
            raise Failure(400, "input_audio could not be decoded", code="bad_audio")
        if samples.shape[0] == 0:
            raise Failure(400, "input_audio is empty", code="bad_audio")
        if samples.shape[0] > MAX_CLIP_SECONDS * rate:
            raise Failure(400, f"input_audio is longer than {MAX_CLIP_SECONDS} seconds", code="bad_audio")
        return self.torch.from_numpy(samples.mean(axis=1)).unsqueeze(0), rate

    def chat(self, plan: dict):
        """Runs a chat turn: `(text, wav bytes or None, prompt tokens, generated tokens, finish)`."""
        if self.state != "ok":
            raise Failure(503, f"the model is {self.state}", "server_error", "model_not_ready")
        from liquid_audio import ChatState

        torch = self.torch
        chat = ChatState(self.processor, dtype=self.dtype)
        chat.new_turn("system")
        chat.add_text(plan["system"])
        chat.end_turn()
        for role, text, clips in plan["turns"]:
            chat.new_turn(role)
            for data in clips:
                wave, rate = self.load_clip(data)
                chat.add_audio(wave, rate)
            if text:
                chat.add_text(text)
            chat.end_turn()
        chat.new_turn("assistant")
        prompt_tokens = int(chat.text.numel()) if hasattr(chat, "text") else 0
        texts, codes, generated, finish = [], [], 0, "stop"
        kw = {"max_new_tokens": plan["max_tokens"], "audio_temperature": 1.0, "audio_top_k": 4}
        if plan["temperature"]:
            kw["text_temperature"] = plan["temperature"]
        started = time.perf_counter()
        with self.lock, torch.no_grad():
            for t in self.model.generate_interleaved(**chat, **kw):
                generated += 1
                if t.numel() == 1:
                    texts.append(t)
                else:
                    if not plan["speech"]:
                        break
                    codes.append(t)
            else:
                finish = "length" if generated >= plan["max_tokens"] else "stop"
            text = ""
            if texts:
                text = self.processor.text.decode(torch.cat([t.reshape(-1) for t in texts]))
                text = text.replace("<|text_end|>", "").strip()
            wav = None
            if plan["speech"] and len(codes) > 1:
                # The last frame is the end-of-audio code.
                audio_codes = torch.stack(codes[:-1], 1).unsqueeze(0)
                samples = self.processor.decode(audio_codes).float().cpu()[0].clamp(-1.0, 1.0)
                wav = wav_bytes((samples * 32767.0).to(torch.int16).numpy().astype("<i2").tobytes())
        print(
            f"liquid-audio: chat {generated} tokens in {time.perf_counter() - started:.1f}s",
            file=sys.stderr,
            flush=True,
        )
        return text, wav, prompt_tokens, generated, finish

    def speak(self, request: dict):
        if self.state != "ok":
            raise Failure(503, f"the model is {self.state}", "server_error", "model_not_ready")
        text = request.get("input")
        if not isinstance(text, str) or not text.strip():
            raise Failure(400, "`input` must not be empty", code="input_empty")
        if len(text) > 4096:
            raise Failure(400, "`input` is longer than 4096 characters", code="input_too_long")
        fmt = str(request.get("response_format") or "wav").lower()
        if fmt not in ("wav", "pcm"):
            raise Failure(400, f"`response_format` `{fmt}` is not served by this worker", code="format_unavailable")
        voice = voice_key(request.get("voice"))
        started = time.perf_counter()
        with self.lock:
            waves = [self.synthesize_part(part, voice) for part in split_text(text)]
        torch = self.torch
        samples = torch.cat(waves).clamp(-1.0, 1.0)
        pcm16 = (samples * 32767.0).to(torch.int16).numpy().astype("<i2").tobytes()
        elapsed = time.perf_counter() - started
        seconds = len(pcm16) / 2 / SAMPLE_RATE
        body = pcm16 if fmt == "pcm" else wav_bytes(pcm16)
        return body, ("audio/wav" if fmt == "wav" else "audio/pcm"), seconds, elapsed


def make_handler(model: Model, key: str, origins: frozenset):
    expected = f"Bearer {key}".encode()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"
        server_version = "harness-liquid-audio"

        def log_message(self, *_):
            pass

        def address_string(self):
            return "local"

        def answer(self, status, body, cors=(), content_type="application/json", extra=()):
            data = body if isinstance(body, bytes) else json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(data)))
            for name, value in tuple(cors) + tuple(extra):
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(data)

        def stream(self, done, cors=()):
            data = b"".join(completion_events(done))
            self.answer(200, data, cors, "text/event-stream", (("Cache-Control", "no-cache"),))

        def fail(self, f: Failure, cors=()):
            self.answer(f.status, {"error": {"message": str(f), "type": f.kind, "code": f.code}}, cors)

        def cors(self):
            origin = self.headers.get("Origin")
            if origin is None:
                return ()
            if origin not in origins:
                raise Failure(403, "Origin not allowed.", "permission_error", "origin_not_allowed")
            return (("Access-Control-Allow-Origin", origin), ("Vary", "Origin"))

        def guard(self):
            cors = self.cors()
            sent = self.headers.get("Authorization", "").encode("latin-1", "replace")
            if not hmac.compare_digest(sent, expected):
                raise Failure(401, "invalid API key", "authentication_error", "invalid_api_key")
            return cors

        def do_OPTIONS(self):
            try:
                cors = self.cors()
            except Failure as f:
                self.fail(f)
                return
            self.send_response(204)
            for name, value in tuple(cors) + (
                ("Access-Control-Allow-Methods", "GET, POST"),
                ("Access-Control-Allow-Headers", "authorization, content-type"),
                ("Access-Control-Max-Age", "600"),
                ("Content-Length", "0"),
            ):
                self.send_header(name, value)
            self.end_headers()

        def do_GET(self):
            cors = ()
            try:
                if self.path == "/health":
                    cors = self.cors()
                    status = {"ok": 200, "loading": 503}.get(model.state, 500)
                    body = {"status": model.state}
                    if model.error:
                        body["error"] = model.error
                    self.answer(status, body, cors)
                    return
                cors = self.guard()
                if self.path == "/v1/models":
                    self.answer(
                        200,
                        {"object": "list", "data": [{"id": model.name, "object": "model", "owned_by": "liquid-audio"}]},
                        cors,
                    )
                    return
                raise Failure(404, f"no route {self.path}", code="not_found")
            except Failure as f:
                self.fail(f, cors)

        def do_POST(self):
            cors = ()
            try:
                cors = self.guard()
                if self.path not in ("/v1/audio/speech", "/v1/chat/completions"):
                    raise Failure(404, f"no route {self.path}", code="not_found")
                try:
                    length = int(self.headers.get("Content-Length") or 0)
                except ValueError:
                    length = -1
                if length < 0 or length > MAX_BODY_BYTES:
                    self.close_connection = True
                    raise Failure(413, f"the request body must have a length of at most {MAX_BODY_BYTES} bytes")
                try:
                    request = json.loads(self.rfile.read(length) or b"{}")
                except ValueError:
                    raise Failure(400, "the request body is not JSON")
                if not isinstance(request, dict):
                    raise Failure(400, "the request body must be a JSON object")
                if self.path == "/v1/chat/completions":
                    plan = chat_plan(request)
                    text, wav, prompt_tokens, generated, finish = model.chat(plan)
                    done = completion(model.name, text, wav, prompt_tokens, generated, finish)
                    if plan["stream"]:
                        self.stream(done, cors)
                    else:
                        self.answer(200, done, cors)
                    return
                body, content_type, seconds, elapsed = model.speak(request)
                self.answer(
                    200,
                    body,
                    cors,
                    content_type,
                    (
                        ("x-harness-audio-seconds", f"{seconds:.3f}"),
                        ("x-harness-generation-seconds", f"{elapsed:.3f}"),
                    ),
                )
            except Failure as f:
                self.fail(f, cors)
            except Exception as e:  # a failed generation answers, it does not drop the connection
                self.fail(Failure(500, str(e), "server_error", "tts_failed"), cors)

    return Handler


if hasattr(socketserver, "UnixStreamServer"):  # not on Windows, where --uds is not used

    class UnixServer(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
        daemon_threads = True


def main(argv=None):
    args = parse_args(argv)
    key = os.environ.get("HARNESS_WORKER_API_KEY", "")
    if not key:
        raise SystemExit("HARNESS_WORKER_API_KEY is not set; refusing to start")
    origins = frozenset(o for o in os.environ.get("HARNESS_WORKER_ORIGINS", "").split(",") if o)
    model = Model(args)
    threading.Thread(target=model.load, name="load", daemon=True).start()
    handler = make_handler(model, key, origins)
    if args.uds:
        if os.path.exists(args.uds):
            os.unlink(args.uds)
        server = UnixServer(args.uds, handler)
    else:
        server = ThreadingHTTPServer((args.host, args.port), handler)
        server.daemon_threads = True
    server.serve_forever()


if __name__ == "__main__":
    main()
