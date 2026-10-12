"""Create (or recreate) the Harness Studio model definitions the suite uses, through the Control API."""
import os
import sys

import httpx

from backends import GGUF, HARNESS_DATA

API = "http://127.0.0.1:11500/api/harness/v1"
H = {"Authorization": "Bearer " + (HARNESS_DATA / "api-token").read_text().strip()}
EQ = {"cache_type_k": "f16", "cache_type_v": "f16", "context_size": 32768, "flash_attn": True, "gpu_layers": 999,
      "parallel_slots": 1}
DEFS = [
    ("bench-4b-default", GGUF["4b"], {}),
    ("bench-4b-equal", GGUF["4b"], EQ),
    ("bench-4b-equal4", GGUF["4b"], {**EQ, "parallel_slots": 4}),
    ("bench-14b-default", GGUF["14b"], {}),
    ("bench-14b-equal", GGUF["14b"], EQ),
    ("bench-14b-ctx65536", GGUF["14b"], {"context_size": 65536}),
]


def main() -> int:
    existing = {d["alias"]: d["id"] for d in httpx.get(API + "/definitions", headers=H).json()}
    for alias, path, load in DEFS:
        if alias in existing:
            httpx.delete(f"{API}/definitions/{existing[alias]}", headers=H)
        r = httpx.post(API + "/definitions", headers=H, json={"id": "x", "alias": alias, "base": {"kind": "local", "path": path}, "load": load})
        print(alias, r.status_code)
    return 0


def remove() -> None:
    for d in httpx.get(API + "/definitions", headers=H).json():
        if d["alias"].startswith("bench-"):
            print("delete", d["alias"], httpx.delete(f"{API}/definitions/{d['id']}", headers=H).status_code)


if __name__ == "__main__":
    sys.exit(remove() if "--remove" in sys.argv else main())
