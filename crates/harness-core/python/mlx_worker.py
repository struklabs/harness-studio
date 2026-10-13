"""Harness MLX worker: mlx-lm's OpenAI-compatible server for one local model folder.

    <env python> mlx_worker.py --model <dir> --port <port> [--alias <name>] [mlx_lm.server options]

What it adds to `mlx_lm.server`:

* With HARNESS_WORKER_API_KEY in the environment every route but /health and the CORS
  preflight needs "Authorization: Bearer <key>" (constant-time comparison); the key is removed
  from the environment once read and never logged.
* It listens on 127.0.0.1 only, whatever --host says.
* The model is the folder given here: a request's "model" or "adapters" never loads another
  path, and /v1/models lists that one model (mlx-lm would scan the Hugging Face cache).
* Remote code is never trusted (--trust-remote-code is refused) and Hugging Face Hub access is
  off (HF_HUB_OFFLINE=1, set by Harness too).
* No cross-origin browser access (the gateway is the way in).
"""

import hmac
import json
import os
import sys
import time

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

KEY = os.environ.pop("HARNESS_WORKER_API_KEY", None) or None
HOST = "127.0.0.1"


def _split_args(argv):
    """`--alias` is ours; the rest goes to mlx_lm.server with Harness's bind and policy."""
    alias = None
    out = []
    it = iter(argv)
    for arg in it:
        flag = arg.split("=", 1)[0]
        if flag == "--alias":
            alias = arg.split("=", 1)[1] if "=" in arg else next(it, None)
            continue
        if flag == "--trust-remote-code":
            sys.exit("mlx_worker: --trust-remote-code is not allowed")
        if flag in ("--host", "--allowed-origins", "--adapter-path", "--draft-model"):
            if "=" not in arg:
                next(it, None)
            continue
        out.append(arg)
    return alias, out + ["--host", HOST, "--allowed-origins", "harness:none"]


def main(argv=None):
    alias, args = _split_args(sys.argv[1:] if argv is None else argv)

    from mlx_lm import server

    class Handler(server.APIHandler):
        def _authorized(self):
            if KEY is None:
                return True
            auth = self.headers.get("Authorization", "")
            return hmac.compare_digest(auth.encode(), f"Bearer {KEY}".encode())

        def _refuse(self):
            body = json.dumps(
                {
                    "error": {
                        "message": "Missing or invalid API key.",
                        "type": "authentication_error",
                        "code": "invalid_api_key",
                    }
                }
            ).encode()
            self.send_response(401)
            self.send_header("Content-type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_POST(self):
            if not self._authorized():
                return self._refuse()
            return super().do_POST()

        def do_GET(self):
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
