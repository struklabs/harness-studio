"""Harness MLX worker: mlx-lm's OpenAI-compatible server for one local model folder.

    <env python> mlx_worker.py --model <dir> --port <port> [--alias <name>] [mlx_lm.server options]

What it adds to `mlx_lm.server`:

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise): every route but
  /health and the CORS preflight needs "Authorization: Bearer <key>" (constant-time comparison
  of bytes); the key is removed from the environment once read and never logged. /health says
  only {"status": ...}.
* Proxy variables are cleared.
* Flags Harness owns (and their abbreviations) are refused; abbreviations are off in
  mlx_lm's parser.
* It listens on 127.0.0.1 only, whatever --host says.
* The model is the folder given here: a request's "model" or "adapters" never loads another
  path, and /v1/models lists that one model (mlx-lm would scan the Hugging Face cache).
* Remote code is never trusted (--trust-remote-code is refused) and Hugging Face Hub access is
  off (HF_HUB_OFFLINE=1, set by Harness too).
* Browsers: only the origins in HARNESS_WORKER_ORIGINS (the Harness window and the user's CORS
  origins) get CORS headers; their preflight is answered here (it carries no key). A request
  with any other Origin is refused (403) before anything else, so no web page can drive it.
"""

import hmac
import json
import os
import sys
import time

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
# No proxy: nothing here should reach the network, and a proxy would carry it there.
for _var in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
    os.environ.pop(_var, None)

KEY = os.environ.pop("HARNESS_WORKER_API_KEY", None) or None
ORIGINS = frozenset(o for o in os.environ.pop("HARNESS_WORKER_ORIGINS", "").split(",") if o)
HOST = "127.0.0.1"

# Flags Harness owns: never accepted from the command line, nor any abbreviation of them
# (argparse would expand `--trust` to `--trust-remote-code`; abbreviations are also turned off
# in mlx_lm's parser, see `main`).
REFUSED = ("--trust-remote-code", "--host", "--allowed-origins", "--adapter-path", "--draft-model")
# Flags Harness gives once (a second one would override it).
ONCE = ("--model", "--port", "--alias")


def _split_args(argv):
    """`--alias` is ours; the rest goes to mlx_lm.server with Harness's bind and policy.
    Exits on a refused flag, an abbreviation of a flag Harness owns, or a repeated one."""
    alias = None
    out = []
    seen = set()
    it = iter(argv)
    for arg in it:
        if not arg.startswith("--"):
            out.append(arg)
            continue
        flag = arg.split("=", 1)[0]
        owned = REFUSED + ONCE
        if flag in REFUSED or (flag not in owned and any(o.startswith(flag) for o in owned)):
            sys.exit(f"mlx_worker: {flag} is not allowed")
        if flag in ONCE:
            if flag in seen:
                sys.exit(f"mlx_worker: {flag} is given twice")
            seen.add(flag)
        if flag == "--alias":
            alias = arg.split("=", 1)[1] if "=" in arg else next(it, None)
            continue
        out.append(arg)
    return alias, out + ["--host", HOST, "--allowed-origins", "harness:none"]


def main(argv=None):
    if KEY is None:
        sys.exit("mlx_worker: HARNESS_WORKER_API_KEY is required")
    alias, args = _split_args(sys.argv[1:] if argv is None else argv)

    import argparse

    from mlx_lm import server

    # mlx_lm builds its parser inside main(): no abbreviated flags there (argparse's own
    # __init__ looks its class up by name, so the method is wrapped, not the class replaced).
    parser_init = argparse.ArgumentParser.__init__

    def strict_init(self, *a, **k):
        k["allow_abbrev"] = False
        parser_init(self, *a, **k)

    argparse.ArgumentParser.__init__ = strict_init

    class Handler(server.APIHandler):
        def _authorized(self):
            auth = self.headers.get("Authorization", "")
            return hmac.compare_digest(auth.encode(), f"Bearer {KEY}".encode())

        def _foreign(self):
            origin = self.headers.get("Origin")
            return origin is not None and origin not in ORIGINS

        def _set_cors_headers(self):
            # mlx_lm's own sends "*" headers; only an allowed origin gets any.
            origin = self.headers.get("Origin")
            if origin in ORIGINS:
                self.send_header("Access-Control-Allow-Origin", origin)
                self.send_header("Vary", "Origin")

        def _refuse(self, status=401, message="Missing or invalid API key.",
                    kind="authentication_error", code="invalid_api_key"):
            body = json.dumps({"error": {"message": message, "type": kind, "code": code}}).encode()
            self.send_response(status)
            self.send_header("Content-type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            if status != 403:
                self._set_cors_headers()
            self.end_headers()
            self.wfile.write(body)

        def _forbid(self):
            return self._refuse(403, "Origin not allowed.", "permission_error", "origin_not_allowed")

        def do_OPTIONS(self):
            if self.headers.get("Origin") is None:
                return super().do_OPTIONS()
            if self.headers.get("Origin") not in ORIGINS:
                return self._forbid()
            self.send_response(204)
            self._set_cors_headers()
            self.send_header("Access-Control-Allow-Methods", "GET, POST")
            self.send_header("Access-Control-Allow-Headers", "authorization, content-type")
            self.send_header("Access-Control-Max-Age", "600")
            self.send_header("Content-Length", "0")
            self.end_headers()

        def do_POST(self):
            if self._foreign():
                return self._forbid()
            if not self._authorized():
                return self._refuse()
            return super().do_POST()

        def do_GET(self):
            if self._foreign():
                return self._forbid()
            if self.path != "/health" and not self._authorized():
                return self._refuse()
            return super().do_GET()

        def handle_models_request(self):
            body = json.dumps(
                {
                    "object": "list",
                    "data": [
                        {
                            "id": alias or "default_model",
                            "object": "model",
                            "created": self.created,
                            "owned_by": "harness",
                        }
                    ],
                }
            ).encode()
            self._set_completion_headers(200)
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, fmt, *a):
            # Request lines only (no headers, so never the key).
            sys.stderr.write("%s %s\n" % (time.strftime("%H:%M:%S"), fmt % a))

    # The folder given on the command line is the only model.
    load = server.ModelProvider.load

    def pinned_load(self, model_path, adapter_path=None, draft_model_path=None):
        return load(self, "default_model", None, "default_model")

    server.ModelProvider.load = pinned_load

    def run(host, port, model_provider, server_class=server.ThreadingHTTPServer, handler_class=None):
        group = server.mx.distributed.init()
        prompt_cache = server.LRUPromptCache(model_provider.cli_args.prompt_cache_size)
        generator = server.ResponseGenerator(model_provider, prompt_cache)
        if group.rank() == 0:
            server._run_http_server(HOST, port, generator, server_class, Handler)
        else:
            generator.join()

    server.run = run
    sys.argv = ["mlx_lm.server"] + args
    server.main()


if __name__ == "__main__":
    main()
