"""Shared helpers: OpenAI-compatible streaming client, prompt builders, GPU memory sampling."""
from __future__ import annotations

import json
import random
import statistics
import subprocess
import time
import uuid
from dataclasses import dataclass, field
from typing import Any

import httpx

SEED = 42

# ---------------------------------------------------------------------------
# Endpoint
# ---------------------------------------------------------------------------


@dataclass
class Endpoint:
    base_url: str  # e.g. http://127.0.0.1:1234/v1
    model: str  # the "model" string to send
    api_key: str | None = None
    info: dict = field(default_factory=dict)  # effective settings recorded at load time

    def headers(self) -> dict:
        h = {"Content-Type": "application/json"}
        if self.api_key:
            h["Authorization"] = f"Bearer {self.api_key}"
        return h


# ---------------------------------------------------------------------------
# Streaming chat
# ---------------------------------------------------------------------------


def stream_chat(ep: Endpoint, messages: list[dict], max_tokens: int, tools: list[dict] | None = None,
                timeout: float = 900.0, extra: dict | None = None) -> dict:
    """One streamed chat completion. Returns timing + usage + text. Never raises on HTTP errors."""
    body: dict[str, Any] = {
        "model": ep.model,
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": 0,
        "seed": SEED,
        "stream": True,
        "stream_options": {"include_usage": True},
    }
    if tools:
        body["tools"] = tools
    if extra:
        body.update(extra)
    res: dict[str, Any] = {"ok": False, "text": "", "usage": None, "timings": None}
    t0 = time.perf_counter()
    t_first = None  # first real token (content / tool call / reasoning)
    t_byte = None  # first SSE data line
    t_last = None
    n_chunks = 0
    tool_calls: dict[int, dict] = {}
    try:
        with httpx.stream("POST", ep.base_url + "/chat/completions", json=body, headers=ep.headers(),
                          timeout=httpx.Timeout(timeout, connect=15)) as r:
            if r.status_code != 200:
                res["error"] = f"HTTP {r.status_code}: {r.read().decode('utf-8', 'replace')[:500]}"
                res["wall"] = time.perf_counter() - t0
                return res
            for line in r.iter_lines():
                if not line.startswith("data:"):
                    continue
                data = line[5:].strip()
                now = time.perf_counter()
                if t_byte is None:
                    t_byte = now
                if data == "[DONE]":
                    break
                try:
                    ch = json.loads(data)
                except json.JSONDecodeError:
                    continue
                if ch.get("error"):
                    res["error"] = json.dumps(ch["error"])[:500]
                    continue
                if ch.get("usage"):
                    res["usage"] = ch["usage"]
                if ch.get("timings"):
                    res["timings"] = ch["timings"]
                for c in ch.get("choices") or []:
                    d = c.get("delta") or {}
                    piece = d.get("content") or d.get("reasoning_content") or d.get("reasoning") or ""
                    tcs = d.get("tool_calls") or []
                    if piece or tcs:
                        if t_first is None:
                            t_first = now
                        t_last = now
                        n_chunks += 1
                    res["text"] += piece
                    for tc in tcs:
                        i = tc.get("index", 0)
                        cur = tool_calls.setdefault(i, {"name": "", "arguments": ""})
                        fn = tc.get("function") or {}
                        cur["name"] += fn.get("name") or ""
                        cur["arguments"] += fn.get("arguments") or ""
                    if c.get("finish_reason"):
                        res["finish_reason"] = c["finish_reason"]
    except Exception as e:  # noqa: BLE001 - benchmark must record, not crash
        res["error"] = f"{type(e).__name__}: {e}"[:500]
        res["wall"] = time.perf_counter() - t0
        return res
    t_end = time.perf_counter()
    res["wall"] = t_end - t0
    res["ttfb"] = (t_byte - t0) if t_byte else None
    res["ttft"] = (t_first - t0) if t_first else None
    res["tool_calls"] = [tool_calls[k] for k in sorted(tool_calls)]
    u = res["usage"] or {}
    ctoks = u.get("completion_tokens") or n_chunks
    res["completion_tokens"] = ctoks
    res["prompt_tokens"] = u.get("prompt_tokens")
    if t_first and t_last and t_last > t_first and ctoks and ctoks > 1:
        res["decode_tps"] = (ctoks - 1) / (t_last - t_first)
    if res["timings"]:
        tm = res["timings"]
        res["server_decode_tps"] = tm.get("predicted_per_second")
        res["server_prompt_tps"] = tm.get("prompt_per_second")
        res["server_cache_n"] = tm.get("cache_n")
    det = (u.get("prompt_tokens_details") or {})
    if det.get("cached_tokens") is not None:
        res["cached_tokens"] = det["cached_tokens"]
    res["ok"] = "error" not in res and t_first is not None
    return res


def complete(ep: Endpoint, messages: list[dict], tools: list[dict] | None = None, max_tokens: int = 512,
             timeout: float = 600.0, extra: dict | None = None) -> dict:
    """Non-streamed chat completion (used for the tool-call suite)."""
    body: dict[str, Any] = {"model": ep.model, "messages": messages, "max_tokens": max_tokens,
                            "temperature": 0, "seed": SEED, "stream": False}
    if tools:
        body["tools"] = tools
    if extra:
        body.update(extra)
    t0 = time.perf_counter()
    try:
        r = httpx.post(ep.base_url + "/chat/completions", json=body, headers=ep.headers(),
                       timeout=httpx.Timeout(timeout, connect=15))
    except Exception as e:  # noqa: BLE001
        return {"ok": False, "error": f"{type(e).__name__}: {e}"[:400], "wall": time.perf_counter() - t0}
    wall = time.perf_counter() - t0
    if r.status_code != 200:
        return {"ok": False, "error": f"HTTP {r.status_code}: {r.text[:400]}", "wall": wall}
    try:
        j = r.json()
        msg = j["choices"][0]["message"]
    except Exception as e:  # noqa: BLE001
        return {"ok": False, "error": f"bad body: {e}: {r.text[:300]}", "wall": wall}
    return {"ok": True, "message": msg, "usage": j.get("usage"), "wall": wall,
            "finish_reason": j["choices"][0].get("finish_reason")}


def median(xs: list[float | None]) -> float | None:
    v = [x for x in xs if x is not None]
    return statistics.median(v) if v else None


# ---------------------------------------------------------------------------
# Deterministic filler text (token counts are calibrated, see calibrate.py output in README)
# ---------------------------------------------------------------------------

_ADJ = "quiet old bright narrow heavy distant careful pale rapid golden hollow gentle crooked frozen modest vivid".split()
_NOUN = ("harbor lantern orchard engine ledger meadow compass bridge archive window valley kettle signal "
         "garden market tunnel statue river anchor cabinet marble thread furnace island mirror pencil").split()
_VERB = "carried watched measured repaired followed painted counted lifted ignored described crossed polished".split()
_PLACE = "station village canal workshop plateau harbor library cellar terrace foundry".split()

# Measured with Qwen3 / Qwen2.5 tokenizer (same vocabulary): characters per token for this generator.
CHARS_PER_TOKEN = 4.54


def filler(n_tokens: int, seed: int) -> str:
    rng = random.Random(seed)
    out: list[str] = []
    total = 0
    target = int(n_tokens * CHARS_PER_TOKEN)
    while total < target:
        s = (f"The {rng.choice(_ADJ)} {rng.choice(_NOUN)} {rng.choice(_VERB)} the {rng.choice(_ADJ)} "
             f"{rng.choice(_NOUN)} near the {rng.choice(_PLACE)}, and {rng.randint(2, 97)} people noticed it. ")
        out.append(s)
        total += len(s)
    return "".join(out)


def nonce() -> str:
    return uuid.uuid4().hex[:12]


# ---------------------------------------------------------------------------
# GPU memory (Windows performance counters; adapter with the most dedicated usage = the dGPU)
# ---------------------------------------------------------------------------

_PS = (r"$d=Get-Counter '\GPU Adapter Memory(*)\Dedicated Usage','\GPU Adapter Memory(*)\Shared Usage' "
       r"| % { $_.CounterSamples } | % { '{0}|{1}' -f $_.Path,$_.CookedValue }; $d")


def gpu_mem() -> dict:
    """Dedicated + shared GPU memory (MiB) of the busiest adapter. Shared growth = spill into system RAM."""
    try:
        out = subprocess.run(["powershell", "-NoProfile", "-Command", _PS], capture_output=True, text=True,
                             timeout=30).stdout
    except Exception as e:  # noqa: BLE001
        return {"error": str(e)}
    ded: dict[str, float] = {}
    sh: dict[str, float] = {}
    for line in out.splitlines():
        if "|" not in line:
            continue
        path, val = line.rsplit("|", 1)
        try:
            v = float(val)
        except ValueError:
            continue
        key = path.split("(")[1].split(")")[0]
        (ded if "dedicated usage" in path.lower() else sh)[key] = v
    if not ded:
        return {"error": "no counters"}
    k = max(ded, key=lambda x: ded[x])
    return {"dedicated_mib": round(ded[k] / 2**20), "shared_mib": round(sh.get(k, 0) / 2**20)}
