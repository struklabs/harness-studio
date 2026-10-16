"""Harness Studio transcribe.cpp worker: serves one transcribe.cpp speech recognition GGUF over
HTTP with the transcribe-cpp binding.

Started by Harness from its managed environment (`<data>/bin/pyenv/envs/transcribe-cpp*`), one
process per model, on a loopback port or a Unix socket (Linux runs it without network, see
`pyenv::isolation`). The model is a local file: nothing is downloaded. Only the standard library
and the binding are used.

Endpoints
  GET  /health                     {"status":"ok"} | 503 {"status":"loading"} | 500 {"status":"error"}
  GET  /v1/models
  POST /v1/audio/transcriptions    multipart: "file" (16 kHz mono 16-bit PCM WAV; Harness converts
                                   the caller's audio), "language", "task" (transcribe | translate),
                                   "timestamps" (none | segment). Answers
                                   {"text", "language", "duration", "segments": [{"start", "end", "text"}]}.

Errors use the OpenAI shape {"error": {"message", "type", "code"}}. Every route but /health needs
"Authorization: Bearer <HARNESS_WORKER_API_KEY>"; the worker refuses to start without that key.
Browsers: only the origins in HARNESS_WORKER_ORIGINS get CORS headers and a preflight answer; any
other Origin is refused with 403.

One run at a time: transcribe.cpp computes one request per model (the binding locks too), so
concurrent requests wait their turn.
"""

from __future__ import annotations

import argparse
import hmac
import io
import json
import os
import socketserver
import sys
import threading
import wave
from array import array
from email.parser import BytesParser
from email.policy import HTTP
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

#: Largest request body: 16 kHz mono 16-bit PCM is 1.9 MB a minute, so about 70 minutes.
MAX_BODY_BYTES = 128 * 1024 * 1024
SAMPLE_RATE = 16000
BACKENDS = ("auto", "cpu", "cuda", "vulkan", "metal")


def parse_args(argv=None):
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True, help="transcribe.cpp GGUF file")
    p.add_argument("--alias", default=None)
    p.add_argument("--host", default="127.0.0.1")
    p.add_argument("--port", type=int, default=0)
    p.add_argument("--uds", default=None, help="listen on this Unix socket instead of a port")
    p.add_argument("--backend", default="auto", choices=BACKENDS)
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


def pcm_of_wav(data: bytes) -> array:
    """The float32 samples of a 16 kHz mono 16-bit PCM WAV (what Harness sends)."""
    try:
        with wave.open(io.BytesIO(data), "rb") as w:
            channels, width, rate = w.getnchannels(), w.getsampwidth(), w.getframerate()
            frames = w.readframes(w.getnframes())
    except (wave.Error, EOFError) as e:
        raise Failure(400, f"the audio is not a WAV file: {e}", code="invalid_audio")
    if channels != 1 or width != 2 or rate != SAMPLE_RATE:
        raise Failure(
            400,
            f"the audio must be 16 kHz mono 16-bit PCM (got {rate} Hz, {channels} channels, {width * 8}-bit)",
            code="invalid_audio",
        )
    pcm16 = array("h")
    pcm16.frombytes(frames)
    if sys.byteorder == "big":
        pcm16.byteswap()
    return array("f", (s / 32768.0 for s in pcm16))


def parse_form(content_type: str, body: bytes) -> dict:
    """The fields of a multipart/form-data body: name -> bytes."""
    if not content_type.lower().startswith("multipart/form-data"):
        raise Failure(400, "the request must be multipart/form-data")
    head = f"Content-Type: {content_type}\r\nMIME-Version: 1.0\r\n\r\n".encode("latin-1")
    message = BytesParser(policy=HTTP).parsebytes(head + body)
    if not message.is_multipart():
        raise Failure(400, "the multipart body could not be read")
    fields = {}
    for part in message.iter_parts():
        name = part.get_param("name", header="content-disposition")
        if name:
            fields[name] = part.get_payload(decode=True) or b""
    return fields


def text_field(fields: dict, name: str):
    value = fields.get(name)
    if value is None:
        return None
    value = value.decode("utf-8", "replace").strip()
    return value or None


class Model:
    def __init__(self, args):
        self.args = args
        self.name = args.alias or os.path.splitext(os.path.basename(args.model))[0]
        self.state = "loading"
        self.error = None
        self.model = None
        self.lock = threading.Lock()

    def load(self):
        try:
            import transcribe_cpp

            self.model = transcribe_cpp.Model(self.args.model, backend=self.args.backend)
            self.state = "ok"
            print(
                f"transcribe_cpp {transcribe_cpp.__version__}: {self.model.arch} on {self.model.backend}",
                file=sys.stderr,
                flush=True,
            )
        except Exception as e:  # the load error is what /health and the log say
            self.error = str(e)
            self.state = "error"
            print(f"load failed: {e}", file=sys.stderr, flush=True)

    def transcribe(self, fields: dict) -> dict:
        import transcribe_cpp

        if self.state != "ok":
            raise Failure(503, f"the model is {self.state}", "server_error", "model_not_ready")
        audio = fields.get("file")
        if not audio:
            raise Failure(400, "the request has no audio `file`")
        pcm = pcm_of_wav(audio)
        if not pcm:
            raise Failure(400, "the audio is empty", code="invalid_audio")
        task = text_field(fields, "task") or "transcribe"
        if task not in ("transcribe", "translate"):
            raise Failure(400, f"unknown task `{task}`")
        caps = self.model.capabilities
        if task == "translate" and not caps.supports_translate:
            raise Failure(400, "this model cannot translate", code="translate_unsupported")
        timestamps = text_field(fields, "timestamps") or "none"
        if timestamps not in ("none", "segment"):
            raise Failure(400, f"unknown timestamps `{timestamps}`")
        if timestamps == "segment" and caps.max_timestamp_kind == "none":
            timestamps = "none"
        language = text_field(fields, "language")
        try:
            with self.lock, self.model.session() as session:
                result = session.run(pcm, task=task, language=language, timestamps=timestamps)
        except transcribe_cpp.InputTooLong as e:
            raise Failure(400, f"the audio is longer than this model takes: {e}", code="input_too_long")
        except transcribe_cpp.InputTooShort as e:
            raise Failure(400, f"the audio is too short: {e}", code="input_too_short")
        except (transcribe_cpp.InvalidArgument, transcribe_cpp.UnsupportedRequest) as e:
            raise Failure(400, str(e), code="unsupported_request")
        except transcribe_cpp.OutOfMemory as e:
            raise Failure(507, str(e), "server_error", "out_of_memory")
        except transcribe_cpp.TranscribeError as e:
            raise Failure(500, str(e), "server_error", "transcribe_failed")
        segments = []
        if result.timestamp_kind != "none":
            segments = [
                {"start": s.t0_ms / 1000, "end": s.t1_ms / 1000, "text": s.text.strip()}
                for s in result.segments
            ]
        return {
            "text": result.text.strip(),
            "language": result.language or language or "",
            "duration": len(pcm) / SAMPLE_RATE,
            "segments": segments,
        }


def make_handler(model: Model, key: str, origins: frozenset):
    expected = f"Bearer {key}".encode()

    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"
        server_version = "harness-transcribe-cpp"

        def log_message(self, *_):
            pass

        def address_string(self):
            return "local"

        def answer(self, status, body, cors=()):
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            for name, value in cors:
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(data)

        def fail(self, f: Failure, cors=()):
            self.answer(f.status, {"error": {"message": str(f), "type": f.kind, "code": f.code}}, cors)

        def cors(self):
            """CORS headers for an allowed browser origin; raises 403 for any other one."""
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
            for name, value in cors + (
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
                    self.answer(200, {"object": "list", "data": [{"id": model.name, "object": "model", "owned_by": "transcribe.cpp"}]}, cors)
                    return
                raise Failure(404, f"no route {self.path}", code="not_found")
            except Failure as f:
                self.fail(f, cors)

        def do_POST(self):
            cors = ()
            try:
                cors = self.guard()
                if self.path != "/v1/audio/transcriptions":
                    raise Failure(404, f"no route {self.path}", code="not_found")
                try:
                    length = int(self.headers.get("Content-Length") or 0)
                except ValueError:
                    length = -1
                if length < 0 or length > MAX_BODY_BYTES:
                    self.close_connection = True
                    raise Failure(413, f"the request body must have a length of at most {MAX_BODY_BYTES} bytes")
                body = self.rfile.read(length)
                fields = parse_form(self.headers.get("Content-Type", ""), body)
                self.answer(200, model.transcribe(fields), cors)
            except Failure as f:
                self.fail(f, cors)

    return Handler


if hasattr(socketserver, "UnixStreamServer"):  # not on Windows, where --uds is not used

    class UnixServer(socketserver.ThreadingMixIn, socketserver.UnixStreamServer):
        daemon_threads = True


def main(argv=None):
    args = parse_args(argv)
    key = os.environ.get("HARNESS_WORKER_API_KEY", "")
    if not key:
        # Fail closed: without its key the worker would serve anyone on this machine.
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
