"""Harness MLX retrieval worker: text embeddings or reranking with one local MLX model folder.

    <env python> mlx_embed_worker.py --model <dir> --port <port> [--alias <name>]
        --task embedding|rerank [--max-tokens N] [--max-inputs N]

Embedding folders (BERT, XLM-RoBERTa, ModernBERT, Qwen3-Embedding, ...) load with
mlx-embeddings; reranking folders (Qwen3-Reranker) load with mlx-lm and score each document by
the model's "yes" against "no" answer, as the model card describes.

* HARNESS_WORKER_API_KEY must be in the environment (it exits otherwise); every route but
  /health needs "Authorization: Bearer <key>" (constant-time comparison of bytes). The key is
  removed from the environment once read and never logged.
* GET /health ({"status": "ok"}), GET /v1/models (the alias) and one POST route by --task:
  /v1/embeddings (OpenAI's shape: {"input": str | [str], "encoding_format": "float" |
  "base64"}) or /v1/rerank (llama.cpp's and Jina's shape: {"query", "documents": [str],
  "top_n", "return_documents"} in, {"results": [{"index", "relevance_score"}]} out, best
  first); the other of the two is 501, every other route is 404.
* Browsers: only the origins in HARNESS_WORKER_ORIGINS get CORS headers; their preflight is
  answered here (it carries no key). A request with any other Origin is refused (403).
* The model is the folder given here, loaded at start; a request never names a file.
* It listens on 127.0.0.1 only; remote code is never trusted; Hugging Face Hub access is off;
  proxy variables are cleared.
* A request cannot run away: it takes at most --max-inputs texts (documents) of at most
  --max-tokens tokens each (longer ones are cut, as llama.cpp does) and a body of MAX_BODY.
"""

import argparse
import asyncio
import base64
import concurrent.futures
import hmac
import json
import math
import os
import struct
import sys
import threading

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
for _var in ("HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"):
    os.environ.pop(_var, None)

KEY = os.environ.pop("HARNESS_WORKER_API_KEY", None) or None
ORIGINS = frozenset(o for o in os.environ.pop("HARNESS_WORKER_ORIGINS", "").split(",") if o)
HOST = "127.0.0.1"
EMBEDDINGS_PATH = "/v1/embeddings"
RERANK_PATH = "/v1/rerank"
MAX_BODY = 16 * 1024 * 1024
DEFAULT_MAX_TOKENS = 512
DEFAULT_MAX_INPUTS = 256
# Texts embedded in one forward pass.
BATCH = 16
# Qwen3-Reranker's prompt (its model card): the score is P("yes") after this.
RERANK_SYSTEM = ("Judge whether the Document meets the requirements based on the Query and the "
                 "Instruct provided. Note that the answer can only be \"yes\" or \"no\".")
RERANK_INSTRUCT = "Given a web search query, retrieve relevant passages that answer the query"
RERANK_PREFIX = f"<|im_start|>system\n{RERANK_SYSTEM}<|im_end|>\n<|im_start|>user\n"
RERANK_SUFFIX = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"

# MLX streams belong to the thread that made them: the model loads and runs on this one.
MODEL_THREAD = concurrent.futures.ThreadPoolExecutor(max_workers=1, thread_name_prefix="model")


def on_model_thread(fn, *args):
    return asyncio.get_running_loop().run_in_executor(MODEL_THREAD, fn, *args)


class BadRequest(Exception):
    pass


def parser():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0], allow_abbrev=False)
    p.add_argument("--model", required=True)
    p.add_argument("--port", type=int, required=True)
    p.add_argument("--alias", default=None)
    p.add_argument("--task", choices=("embedding", "rerank"), required=True)
    p.add_argument("--max-tokens", type=int, default=DEFAULT_MAX_TOKENS)
    p.add_argument("--max-inputs", type=int, default=DEFAULT_MAX_INPUTS)
    return p


def pooling_of(folder):
    """How sentence-transformers pools this model: "cls", "last" or "mean" (its 1_Pooling
    config; BGE models, which ship without it, pool the CLS token)."""
    try:
        with open(os.path.join(folder, "modules.json"), encoding="utf-8") as f:
            modules = json.load(f)
        pooling = next((m for m in modules if m.get("type", "").endswith("Pooling")), None)
        if pooling:
            with open(os.path.join(folder, pooling.get("path", "1_Pooling"), "config.json"),
                      encoding="utf-8") as f:
                cfg = json.load(f)
            if cfg.get("pooling_mode_cls_token"):
                return "cls"
            if cfg.get("pooling_mode_lasttoken"):
                return "last"
            if cfg.get("pooling_mode_mean_tokens"):
                return "mean"
    except (OSError, ValueError, StopIteration):
        pass
    return "cls" if "bge" in os.path.basename(os.path.normpath(folder)).lower() else None


def texts_of(value, what):
    if isinstance(value, str):
        value = [value]
    if not isinstance(value, list) or not value or not all(isinstance(v, str) for v in value):
        raise BadRequest(f"'{what}' must be a string or a non-empty list of strings.")
    return value


def make_embed(folder, max_tokens):
    """`embed(texts)` -> ([vector], tokens) (blocking, on MODEL_THREAD)."""
    import mlx.core as mx
    from mlx_embeddings.utils import load

    model, tokenizer = load(folder, tokenizer_config={"trust_remote_code": False})
    pooling = pooling_of(folder)

    def embed(texts):
        vectors, tokens = [], 0
        for start in range(0, len(texts), BATCH):
            batch = texts[start:start + BATCH]
            enc = tokenizer(batch, return_tensors="np", padding=True, truncation=True,
                            max_length=max_tokens)
            ids = mx.array(enc["input_ids"])
            mask = mx.array(enc["attention_mask"])
            tokens += int(enc["attention_mask"].sum())
            out = model(ids, attention_mask=mask)
            if pooling == "cls" and out.last_hidden_state is not None:
                pooled = out.last_hidden_state[:, 0, :]
                pooled = pooled / mx.maximum(mx.linalg.norm(pooled, axis=-1, keepdims=True), 1e-9)
            else:
                pooled = out.text_embeds
            vectors.extend(pooled.astype(mx.float32).tolist())
        return vectors, tokens

    return embed


def make_rerank(folder, max_tokens):
    """`rerank(query, documents)` -> ([score], tokens) (blocking, on MODEL_THREAD)."""
    import mlx.core as mx
    from mlx_lm import load

    model, tokenizer = load(folder, tokenizer_config={"trust_remote_code": False})
    yes = tokenizer.convert_tokens_to_ids("yes")
    no = tokenizer.convert_tokens_to_ids("no")
    prefix = tokenizer.encode(RERANK_PREFIX, add_special_tokens=False)
    suffix = tokenizer.encode(RERANK_SUFFIX, add_special_tokens=False)

    def rerank(query, documents):
        scores, tokens = [], 0
        query_ids = tokenizer.encode(f"<Instruct>: {RERANK_INSTRUCT}\n<Query>: {query}\n",
                                     add_special_tokens=False)[:max_tokens]
        for doc in documents:
            doc_ids = tokenizer.encode(f"<Document>: {doc}", add_special_tokens=False)[:max_tokens]
            ids = prefix + query_ids + doc_ids + suffix
            tokens += len(ids)
            logits = model(mx.array([ids]))[0, -1, :]
            diff = (logits[yes] - logits[no]).item()
            scores.append(1.0 / (1.0 + math.exp(-diff)))
        return scores, tokens

    return rerank


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
    """Browser access for ORIGINS only (see mlx_audio_worker.cors)."""

    async def app(scope, receive, send):
        if scope["type"] != "http":
            return await inner(scope, receive, send)
        origin = dict(scope.get("headers") or []).get(b"origin")
        if origin is None:
            return await inner(scope, receive, send)
        if origin.decode("latin-1") not in ORIGINS:
            return await respond(send, 403, error(
                "Origin not allowed.", "permission_error", "origin_not_allowed"))
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


def handler_for(task, run, alias, max_inputs):
    """The POST route's handler: parsed JSON body -> (status, payload)."""

    def embeddings(body):
        texts = texts_of(body.get("input"), "input")
        if len(texts) > max_inputs:
            raise BadRequest(f"At most {max_inputs} inputs per request.")
        fmt = body.get("encoding_format") or "float"
        if fmt not in ("float", "base64"):
            raise BadRequest("'encoding_format' must be 'float' or 'base64'.")
        return texts, fmt

    def answer_embeddings(result, fmt):
        vectors, tokens = result
        data = []
        for i, v in enumerate(vectors):
            emb = (base64.b64encode(struct.pack(f"<{len(v)}f", *v)).decode()
                   if fmt == "base64" else v)
            data.append({"object": "embedding", "index": i, "embedding": emb})
        return {"object": "list", "data": data, "model": alias,
                "usage": {"prompt_tokens": tokens, "total_tokens": tokens}}

    def rerank(body):
        query = body.get("query")
        if not isinstance(query, str) or not query:
            raise BadRequest("'query' must be a non-empty string.")
        docs = body.get("documents")
        if isinstance(docs, list):
            docs = [d.get("text") if isinstance(d, dict) else d for d in docs]
        docs = texts_of(docs, "documents")
        if len(docs) > max_inputs:
            raise BadRequest(f"At most {max_inputs} documents per request.")
        top_n = body.get("top_n")
        if top_n is not None and (not isinstance(top_n, int) or isinstance(top_n, bool)
                                  or top_n < 1):
            raise BadRequest("'top_n' must be a positive integer.")
        return query, docs, top_n, bool(body.get("return_documents"))

    def answer_rerank(result, docs, top_n, with_docs):
        scores, tokens = result
        order = sorted(range(len(scores)), key=lambda i: scores[i], reverse=True)
        results = []
        for i in order[:top_n or len(order)]:
            item = {"index": i, "relevance_score": scores[i]}
            if with_docs:
                item["document"] = {"text": docs[i]}
            results.append(item)
        return {"model": alias, "results": results,
                "usage": {"prompt_tokens": tokens, "total_tokens": tokens}}

    one_at_a_time = threading.Lock()

    def locked(*args):
        with one_at_a_time:
            return run(*args)

    async def handle(raw):
        try:
            body = json.loads(raw or b"{}")
            if not isinstance(body, dict):
                raise BadRequest("The body must be a JSON object.")
            if task == "embedding":
                texts, fmt = embeddings(body)
                return 200, answer_embeddings(await on_model_thread(locked, texts), fmt)
            query, docs, top_n, with_docs = rerank(body)
            return 200, answer_rerank(await on_model_thread(locked, query, docs), docs, top_n,
                                      with_docs)
        except (BadRequest, ValueError) as e:
            return 400, error(str(e), "invalid_request_error", "invalid_request")
        except Exception as e:  # noqa: BLE001 - the model failed; the server keeps serving
            print(f"mlx_embed_worker: request failed: {e}", file=sys.stderr)
            return 500, error("The model failed to answer.", "server_error", "model_failed")

    return handle


def app_for(task, run, alias, max_inputs):
    """The ASGI app: /health, /v1/models and the task's POST route."""
    expected = f"Bearer {KEY}".encode()
    path = EMBEDDINGS_PATH if task == "embedding" else RERANK_PATH
    routes = {("GET", "/health"), ("GET", "/v1/models"), ("POST", path)}
    handle = handler_for(task, run, alias, max_inputs)
    entry = {"id": alias, "object": "model", "owned_by": "harness"}

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
        if method == "POST" and route in (EMBEDDINGS_PATH, RERANK_PATH) and route != path:
            # The other retrieval route: this model does not serve it (as llama.cpp says).
            return await respond(send, 501, error(
                f"This model serves {path} only.", "not_implemented", "wrong_endpoint"))
        if (method, route) not in routes:
            return await respond(send, 404, error("Not found.", "not_found", "not_found"))
        if route == "/health":
            return await respond(send, 200, {"status": "ok"})
        headers = dict(scope.get("headers") or [])
        if not hmac.compare_digest(headers.get(b"authorization", b""), expected):
            return await respond(send, 401, error(
                "Missing or invalid API key.", "authentication_error", "invalid_api_key"))
        if route == "/v1/models":
            return await respond(send, 200, {"object": "list", "data": [entry]})
        declared = headers.get(b"content-length", b"")
        if declared.isdigit() and int(declared) > MAX_BODY:
            return await respond(send, 413, error(
                "Request body too large.", "invalid_request_error", "body_too_large"))
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
        status, payload = await handle(b"".join(chunks))
        await respond(send, status, payload)

    return app


def main(argv=None):
    if KEY is None:
        sys.exit("mlx_embed_worker: HARNESS_WORKER_API_KEY is required")
    args = parser().parse_args(argv)
    if not os.path.isdir(args.model):
        sys.exit("mlx_embed_worker: the model must be a local folder")
    if args.max_tokens < 1 or args.max_inputs < 1:
        sys.exit("mlx_embed_worker: --max-tokens and --max-inputs must be positive")
    alias = args.alias or os.path.basename(os.path.normpath(args.model))
    make = make_embed if args.task == "embedding" else make_rerank
    try:
        run = MODEL_THREAD.submit(make, args.model, args.max_tokens).result()
    except Exception as e:  # noqa: BLE001 - a folder the library cannot load
        sys.exit("mlx_embed_worker: %s" % e)
    import uvicorn

    uvicorn.run(cors(app_for(args.task, run, alias, args.max_inputs)), host=HOST, port=args.port,
                log_level="info", access_log=True)


if __name__ == "__main__":
    main()
