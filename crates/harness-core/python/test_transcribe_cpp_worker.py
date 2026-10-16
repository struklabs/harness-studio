"""Checks transcribe_cpp_worker.py against a stand-in transcribe_cpp module (no native library,
no model): python -I test_transcribe_cpp_worker.py <transcribe_cpp_worker.py>. Prints PASS."""

import http.client
import importlib.util
import io
import json
import os
import sys
import threading
import types
import wave
from http.server import ThreadingHTTPServer

# --- a stand-in binding -------------------------------------------------------------------------
tc = types.ModuleType("transcribe_cpp")
tc.__version__ = "0.4.0"


class TranscribeError(RuntimeError):
    pass


for name in ("InputTooLong", "InputTooShort", "InvalidArgument", "UnsupportedRequest", "OutOfMemory"):
    setattr(tc, name, type(name, (TranscribeError,), {}))
tc.TranscribeError = TranscribeError
runs = []


class Session:
    def __enter__(self):
        return self

    def __exit__(self, *_):
        return False

    def run(self, pcm, *, task, language, timestamps):
        runs.append((len(pcm), task, language, timestamps))
        if len(pcm) > 16000 * 5:
            raise tc.InputTooLong("over 5 s")
        seg = types.SimpleNamespace(t0_ms=0, t1_ms=500, text=" hello ")
        return types.SimpleNamespace(
            text=" hello world ",
            language=language or "en",
            timestamp_kind=timestamps,
            segments=(seg,),
        )


class Model:
    arch = "parakeet"
    backend = "cpu"
    capabilities = types.SimpleNamespace(supports_translate=False, max_timestamp_kind="token")

    def __init__(self, path, backend="auto"):
        if not path.endswith(".gguf"):
            raise TranscribeError("not a model")

    def session(self):
        return Session()


tc.Model = Model
sys.modules["transcribe_cpp"] = tc

spec = importlib.util.spec_from_file_location("w", sys.argv[1])
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)


def wav(seconds, rate=16000, channels=1):
    out = io.BytesIO()
    with wave.open(out, "wb") as f:
        f.setnchannels(channels)
        f.setsampwidth(2)
        f.setframerate(rate)
        f.writeframes(b"\x00\x10" * int(rate * seconds) * channels)
    return out.getvalue()


def form(fields):
    boundary = "harnessboundary"
    parts = []
    for name, value in fields.items():
        if isinstance(value, bytes):
            head = f'Content-Disposition: form-data; name="{name}"; filename="a.wav"\r\nContent-Type: audio/wav\r\n\r\n'
            parts.append(f"--{boundary}\r\n".encode() + head.encode() + value + b"\r\n")
        else:
            parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode())
    return f"multipart/form-data; boundary={boundary}", b"".join(parts) + f"--{boundary}--\r\n".encode()


args = w.parse_args(["--model", "m.gguf", "--port", "1", "--alias", "parakeet-q4"])
model = w.Model(args)
model.load()
assert model.state == "ok", model.error
server = ThreadingHTTPServer(("127.0.0.1", 0), w.make_handler(model, "k", frozenset({"tauri://localhost"})))
threading.Thread(target=server.serve_forever, daemon=True).start()
port = server.server_address[1]


def call(method, path, body=None, headers=None):
    c = http.client.HTTPConnection("127.0.0.1", port, timeout=10)
    c.request(method, path, body=body, headers=headers or {})
    r = c.getresponse()
    data = r.read()
    c.close()
    return r.status, dict(r.getheaders()), (json.loads(data) if data else None)


auth = {"Authorization": "Bearer k"}
assert call("GET", "/health")[0] == 200
assert call("GET", "/v1/models")[0] == 401
status, _, body = call("GET", "/v1/models", headers=auth)
assert status == 200 and body["data"][0]["id"] == "parakeet-q4", body
# Browsers: a foreign origin is refused before the key, an allowed one gets CORS headers.
assert call("GET", "/v1/models", headers={**auth, "Origin": "https://evil.example"})[0] == 403
status, headers, _ = call("OPTIONS", "/v1/audio/transcriptions", headers={"Origin": "tauri://localhost"})
assert status == 204 and headers.get("Access-Control-Allow-Origin") == "tauri://localhost", headers

ctype, body = form({"file": wav(1), "language": "en", "timestamps": "segment"})
status, _, out = call("POST", "/v1/audio/transcriptions", body, {**auth, "Content-Type": ctype})
assert status == 200, out
assert out["text"] == "hello world" and out["language"] == "en" and abs(out["duration"] - 1) < 1e-6, out
assert out["segments"] == [{"start": 0.0, "end": 0.5, "text": "hello"}], out
assert runs[-1] == (16000, "transcribe", "en", "segment"), runs

for fields, code in [
    ({"file": wav(1, rate=44100)}, "invalid_audio"),
    ({"file": wav(1, channels=2)}, "invalid_audio"),
    ({"file": b"not a wav"}, "invalid_audio"),
    ({"file": wav(1), "task": "translate"}, "translate_unsupported"),
    ({"file": wav(6)}, "input_too_long"),
]:
    ctype, body = form(fields)
    status, _, out = call("POST", "/v1/audio/transcriptions", body, {**auth, "Content-Type": ctype})
    assert status == 400 and out["error"]["code"] == code, (fields.keys(), status, out)
status, _, out = call("POST", "/v1/audio/transcriptions", b"{}", {**auth, "Content-Type": "application/json"})
assert status == 400, out
assert call("POST", "/v1/other", b"", auth)[0] == 404

# A model that fails to load says so on /health.
broken = w.Model(w.parse_args(["--model", "m.bin", "--port", "1"]))
broken.load()
assert broken.state == "error" and "not a model" in broken.error

# No key, no worker.
os.environ.pop("HARNESS_WORKER_API_KEY", None)
try:
    w.main(["--model", "m.gguf", "--port", "1"])
except SystemExit as e:
    assert "HARNESS_WORKER_API_KEY" in str(e)
else:
    raise AssertionError("started without a key")
server.shutdown()
print("PASS")
