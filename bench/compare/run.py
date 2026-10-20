"""Benchmark entry point: Harness Studio vs LM Studio vs Ollama vs plain llama.cpp.

    python run.py --backends harness,lmstudio,ollama,llamacpp --modes out_of_box,equal
    python run.py --backends ollama --modes equal --metrics speed,tools   # re-run a slice

Raw JSON per (backend, mode, metric) goes to scratch/benchmarks/<date>/raw/. Use summarize.py afterwards.
"""
from __future__ import annotations

import argparse
import os
import concurrent.futures as cf
import json
import platform
import sys
import threading
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")

from backends import LoadError, baseline_ok, make_backends, stop_all  # noqa: E402
from common import Endpoint, complete, filler, gpu_mem, median, nonce, stream_chat  # noqa: E402
from tools_suite import CASES, MANY38, SYSTEM, score_case  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
OUT = REPO / "scratch" / "benchmarks" / os.environ.get("BENCH_RESULTS", "2026-10-11") / "raw"
ALL_METRICS = ["speed", "needle", "tools", "agent", "concurrency", "memory", "switch"]
NEEDLE = "amber-falcon-7421"


def log(*a):
    print(time.strftime("%H:%M:%S"), *a, flush=True)


def save(backend: str, mode: str, model: str, metric: str, data: dict) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    f = OUT / f"{backend}__{mode}__{model}__{metric}.json"
    f.write_text(json.dumps(data, indent=1, ensure_ascii=False), encoding="utf-8")


def slim(r: dict) -> dict:
    keep = ("ok", "error", "ttft", "ttfb", "wall", "decode_tps", "completion_tokens", "prompt_tokens", "server_decode_tps",
            "server_prompt_tps", "server_cache_n", "cached_tokens", "finish_reason")
    d = {k: r[k] for k in keep if k in r}
    d["text_head"] = (r.get("text") or "")[:120]
    return d


# ---------------------------------------------------------------------------
# metrics
# ---------------------------------------------------------------------------

def m_speed(ep: Endpoint, reps: int = 5) -> dict:
    def p500():
        return [{"role": "user", "content": f"[{nonce()}] Read this passage, then write a long, detailed story that "
                 f"continues it (at least 400 words).\n\n{filler(430, 1)}"}]

    def p4k():
        return [{"role": "user", "content": f"[{nonce()}] {filler(3950, 2)}\n\nReply with the single word: done."}]

    out: dict = {"decode": [], "pp4k": []}
    stream_chat(ep, p500(), 64)  # warm-up (discarded)
    for _ in range(reps):
        out["decode"].append(slim(stream_chat(ep, p500(), 256)))
    stream_chat(ep, p4k(), 1)  # warm-up
    for _ in range(3):
        r = stream_chat(ep, p4k(), 1)
        s = slim(r)
        if r.get("ttft") and r.get("prompt_tokens"):
            s["client_pp_tps"] = r["prompt_tokens"] / r["ttft"]
        out["pp4k"].append(s)
    d = out["decode"]
    out["summary"] = {
        "ttft_500_s": median([x.get("ttft") for x in d]),
        "decode_tps": median([x.get("decode_tps") for x in d]),
        "server_decode_tps": median([x.get("server_decode_tps") for x in d]),
        "prompt_tokens_500": median([x.get("prompt_tokens") for x in d]),
        "completion_tokens": median([x.get("completion_tokens") for x in d]),
        "pp4k_tps_client": median([x.get("client_pp_tps") for x in out["pp4k"]]),
        "pp4k_tps_server": median([x.get("server_prompt_tps") for x in out["pp4k"]]),
        "pp4k_prompt_tokens": median([x.get("prompt_tokens") for x in out["pp4k"]]),
        "pp4k_ttft_s": median([x.get("ttft") for x in out["pp4k"]]),
        "errors": [x["error"] for x in d + out["pp4k"] if x.get("error")][:3],
    }
    return out


def m_needle(ep: Endpoint, target_tokens: int = 20000, reps: int = 2) -> dict:
    runs = []
    for i in range(reps):
        needle = f"{NEEDLE.rsplit('-', 1)[0]}-{7421 + i * 0}"  # same needle, nonce goes in a separate sentence
        text = (f"IMPORTANT FACT (run {nonce()}): the secret passphrase is '{NEEDLE}'. Remember it.\n\n"
                f"{filler(target_tokens - 120, 3 + i)}\n\n"
                "Question: what is the secret passphrase mentioned at the very beginning of this message? "
                "Answer with only the passphrase.")
        r = stream_chat(ep, [{"role": "user", "content": text}], 48, timeout=900)
        s = slim(r)
        s["answer_correct"] = NEEDLE in (r.get("text") or "")
        pt = r.get("prompt_tokens")
        if r.get("error"):
            s["verdict"] = "error_reported"
        elif s["answer_correct"]:
            s["verdict"] = "correct"
        elif pt is not None and pt < 0.9 * target_tokens:
            s["verdict"] = "SILENT_TRUNCATION"
        else:
            s["verdict"] = "wrong_answer_no_error"
        runs.append(s)
    return {"target_tokens": target_tokens, "runs": runs, "summary": {"verdicts": [r["verdict"] for r in runs],
            "prompt_tokens_reported": [r.get("prompt_tokens") for r in runs]}}


def m_tools(ep: Endpoint) -> dict:
    res = []
    for case in CASES:
        _cid, _cat, _lang, tools, prompt, _exp = case
        msgs = [{"role": "system", "content": SYSTEM}, {"role": "user", "content": prompt}]
        r = complete(ep, msgs, tools=tools, max_tokens=512, extra={"parallel_tool_calls": True})
        res.append(score_case(case, r))
    n = len(res)
    cats: dict[str, list] = {}
    for r in res:
        cats.setdefault(r["category"], []).append(r["pass"])
    return {"cases": res, "summary": {
        "passed": sum(r["pass"] for r in res), "total": n, "pct": round(100 * sum(r["pass"] for r in res) / n, 1),
        "by_category": {c: f"{sum(v)}/{len(v)}" for c, v in cats.items()},
        "text_format_calls": sum(r["text_format_call"] for r in res),
        "text_format_converted_and_passed": sum(1 for r in res if r["text_format_call"] and r["pass"]),
        "request_errors": sum(1 for r in res if r["reason"].startswith("request_error")),
        "n_tools_many": len(MANY38)}}


def agent_system(tag: str) -> str:
    return (f"[session {tag}] You are an autonomous coding agent working inside a repository. Follow the project "
            "conventions, prefer small safe edits, and explain results briefly.\n\nProject notes:\n" + filler(7800, 11))


def m_agent(ep: Endpoint, reps: int = 3) -> dict:
    runs = []
    for i in range(reps + 1):  # first rep is a discarded warm-up
        sysmsg = {"role": "system", "content": agent_system(nonce())}
        u1 = {"role": "user", "content": "I'm going to ask you about README.md. Reply with exactly: READY."}
        a1 = {"role": "assistant", "content": None, "tool_calls": [
            {"id": "call_1", "type": "function", "function": {"name": "read_file", "arguments": json.dumps({"path": "README.md"})}}]}
        t1 = {"role": "tool", "tool_call_id": "call_1", "content": "README.md contents:\n" + filler(300, 21)}
        a2 = {"role": "assistant", "content": "The README describes a harbor-themed demo project."}
        u2 = {"role": "user", "content": "Thanks. In one short sentence, what did the README describe? Do not call tools."}
        r1 = stream_chat(ep, [sysmsg, u1], 24, tools=MANY38)
        r2 = stream_chat(ep, [sysmsg, u1, a1, t1], 32, tools=MANY38)
        r3 = stream_chat(ep, [sysmsg, u1, a1, t1, a2, u2], 32, tools=MANY38)
        runs.append({"warmup": i == 0, "turn1": slim(r1), "turn2": slim(r2), "turn3": slim(r3)})
    real = [r for r in runs if not r["warmup"]]
    return {"runs": runs, "summary": {
        "turn1_ttft_s": median([r["turn1"].get("ttft") for r in real]),
        "turn2_ttft_s": median([r["turn2"].get("ttft") for r in real]),
        "turn3_ttft_s": median([r["turn3"].get("ttft") for r in real]),
        "turn1_prompt_tokens": median([r["turn1"].get("prompt_tokens") for r in real]),
        "turn3_prompt_tokens": median([r["turn3"].get("prompt_tokens") for r in real]),
        "turn2_cached_tokens": median([r["turn2"].get("server_cache_n") or r["turn2"].get("cached_tokens") for r in real]),
        "turn3_cached_tokens": median([r["turn3"].get("server_cache_n") or r["turn3"].get("cached_tokens") for r in real]),
        "errors": [r[t]["error"] for r in real for t in ("turn1", "turn2", "turn3") if r[t].get("error")][:3]}}


TOPICS = ["a lighthouse keeper", "a desert caravan", "a clockmaker's apprentice", "a mountain train"]


def m_concurrency(ep: Endpoint, reps: int = 3) -> dict:
    def one_round() -> dict:
        barrier = threading.Barrier(4)
        res: list[dict] = [{}] * 4

        def worker(i: int):
            msgs = [{"role": "user", "content": f"[{nonce()}] Write a long, detailed story (at least 400 words) about {TOPICS[i]}."}]
            barrier.wait()
            res[i] = stream_chat(ep, msgs, 256)

        t0 = time.perf_counter()
        with cf.ThreadPoolExecutor(4) as ex:
            list(ex.map(worker, range(4)))
        wall = time.perf_counter() - t0
        toks = sum((r.get("completion_tokens") or 0) for r in res)
        return {"wall_s": wall, "completion_tokens": toks, "aggregate_tps": toks / wall if wall else None,
                "per_request": [slim(r) for r in res],
                "latency_median_s": median([r.get("wall") for r in res]), "latency_max_s": max((r.get("wall") or 0) for r in res),
                "ttft_median_s": median([r.get("ttft") for r in res]), "ttft_max_s": max((r.get("ttft") or 0) for r in res),
                "errors": [r["error"] for r in res if r.get("error")]}

    one_round()  # warm-up
    rounds = [one_round() for _ in range(reps)]
    return {"rounds": rounds, "summary": {
        "aggregate_tps": median([r["aggregate_tps"] for r in rounds]),
        "wall_s": median([r["wall_s"] for r in rounds]),
        "latency_median_s": median([r["latency_median_s"] for r in rounds]),
        "latency_max_s": median([r["latency_max_s"] for r in rounds]),
        "ttft_median_s": median([r["ttft_median_s"] for r in rounds]),
        "ttft_max_s": median([r["ttft_max_s"] for r in rounds]),
        "completion_tokens": median([r["completion_tokens"] for r in rounds]),
        "errors": [e for r in rounds for e in r["errors"]][:3]}}


def decode_probe(ep: Endpoint, reps: int = 3) -> dict:
    runs = []
    for i in range(reps + 1):
        r = stream_chat(ep, [{"role": "user", "content": f"[{nonce()}] Write a long, detailed story (at least 400 words) about a harbor."}], 256)
        if i:
            runs.append(slim(r))
    return {"runs": runs, "decode_tps": median([r.get("decode_tps") for r in runs]),
            "server_decode_tps": median([r.get("server_decode_tps") for r in runs]),
            "errors": [r["error"] for r in runs if r.get("error")][:2]}


# ---------------------------------------------------------------------------
# orchestration
# ---------------------------------------------------------------------------

def run_memory(be, base_mib: int) -> dict:
    """14B with a 65536 context on this tool."""
    stop_all({be.name: be})
    be.stop()
    pre = baseline_ok(base_mib)
    out: dict = {"gpu_before": pre}
    try:
        ep = be.serve("14b", "ctx65536")
    except LoadError as e:
        out["outcome"] = "refused_or_failed"
        out["error"] = str(e)[:1500]
        out["gpu_after"] = gpu_mem()
        return out
    out["outcome"] = "loaded"
    out["info"] = ep.info
    out["gpu_after_load"] = gpu_mem()
    out["decode"] = decode_probe(ep)
    out["gpu_after_run"] = gpu_mem()
    sh_delta = (out["gpu_after_run"].get("shared_mib", 0) or 0) - (pre.get("shared_mib", 0) or 0)
    out["shared_growth_mib"] = sh_delta
    out["spilled_to_shared_memory"] = sh_delta > 1024
    return out


def run_switch(be, mode: str, base_mib: int) -> dict:
    """4B loaded; request the 14B through the API and time the first token."""
    out: dict = {}
    try:
        be.serve("4b", mode)
    except LoadError as e:
        return {"outcome": "setup_failed", "error": str(e)[:800]}
    ep = be.switch_to("14b", mode)
    if ep is None:
        # plain llama-server: one model per process. Report the manual restart cost for reference.
        t0 = time.perf_counter()
        try:
            be.serve("14b", mode)
            out["manual_restart_ready_s"] = round(time.perf_counter() - t0, 2)
        except LoadError as e:
            out["manual_restart_error"] = str(e)[:300]
        out["outcome"] = "not_supported_on_demand"
        out["reason"] = "llama-server serves exactly one model per process; switching needs a kill and restart"
        return out
    msgs = [{"role": "user", "content": f"[{nonce()}] Reply with one short sentence about the sea."}]
    r = stream_chat(ep, msgs, 32, timeout=600)
    out["request"] = slim(r)
    out["info"] = ep.info
    out["outcome"] = "switched" if r.get("ok") else "failed"
    out["ttft_s"] = r.get("ttft")
    out["gpu_after"] = gpu_mem()
    return out


PLAN = {
    "out_of_box": {"4b": ["speed", "needle", "tools", "agent", "concurrency"], "14b": ["speed", "tools"]},
    "equal": {"4b": ["speed", "tools", "agent"], "14b": ["speed", "tools"]},
    "equal4": {"4b": ["concurrency"]},
}
FUNCS = {"speed": m_speed, "needle": m_needle, "tools": m_tools, "agent": m_agent, "concurrency": m_concurrency}


def run_backend_mode(be, mode: str, metrics: list[str], base_mib: int) -> None:
    tag = f"{be.name}/{mode}"
    for m, wanted in PLAN.get(mode, {}).items():
        todo = [x for x in wanted if x in metrics]
        if not todo:
            continue
        stop_all({be.name: be})  # GPU discipline: nothing else may hold VRAM
        be.stop()
        base = baseline_ok(base_mib)
        log(f"== {tag} model={m}: load")
        try:
            ep = be.serve(m, mode)
        except LoadError as e:
            log(f"   LOAD FAILED: {str(e)[:300]}")
            for metric in todo:
                save(be.name, mode, m, metric, {"error": "load_failed", "detail": str(e)[:1500], "gpu_before": base})
            continue
        meta = {"backend": be.name, "mode": mode, "model": m, "endpoint_info": ep.info, "gpu_before": base,
                "gpu_after_load": gpu_mem(), "date": time.strftime("%Y-%m-%d %H:%M:%S")}
        for metric in todo:
            log(f"   {tag} {m} {metric}")
            t0 = time.time()
            try:
                data = FUNCS[metric](ep)
            except Exception as e:  # noqa: BLE001
                data = {"error": f"{type(e).__name__}: {e}"}
            data["meta"] = dict(meta)
            data["meta"]["gpu_after_run"] = gpu_mem()
            data["meta"]["elapsed_s"] = round(time.time() - t0, 1)
            if hasattr(be, "refresh_info"):
                data["meta"]["endpoint_info_after"] = be.refresh_info(ep)
            save(be.name, mode, m, metric, data)
            summ = data.get("summary")
            log("      ->", json.dumps(summ, ensure_ascii=False)[:400] if summ else data.get("error"))
    if "memory" in metrics and mode == "ctx65536":
        log(f"== {tag} memory safety (14B @ 65536)")
        save(be.name, mode, "14b", "memory", run_memory(be, base_mib))
    if "switch" in metrics and mode == "out_of_box":
        log(f"== {tag} model switch 4B -> 14B")
        stop_all({be.name: be})
        be.stop()
        baseline_ok(base_mib)
        save(be.name, mode, "4b-to-14b", "switch", run_switch(be, mode, base_mib))


def env_info(backends) -> dict:
    import httpx
    info = {"date": time.strftime("%Y-%m-%d %H:%M:%S"), "python": platform.python_version(), "httpx": httpx.__version__,
            "os": platform.platform(), "versions": {}}
    for n, b in backends.items():
        try:
            info["versions"][n] = b.version()
        except Exception as e:  # noqa: BLE001
            info["versions"][n] = f"? ({e})"
    return info


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--backends", default="harness,lmstudio,ollama,llamacpp")
    ap.add_argument("--modes", default="out_of_box,equal,equal4,ctx65536")
    ap.add_argument("--metrics", default=",".join(ALL_METRICS))
    ap.add_argument("--baseline-mib", type=int, default=None, help="dedicated GPU MiB used with nothing loaded")
    a = ap.parse_args()
    names = a.backends.split(",")
    # Only the selected tools: stopping an absent one (LM Studio not installed) fails.
    backends = {n: b for n, b in make_backends().items() if n in names}
    metrics = a.metrics.split(",")
    stop_all(backends)
    time.sleep(3)
    gm = gpu_mem()
    base_mib = a.baseline_mib if a.baseline_mib is not None else gm.get("dedicated_mib", 0)
    log("baseline GPU (nothing loaded):", gm)
    OUT.mkdir(parents=True, exist_ok=True)
    env = env_info({n: backends[n] for n in names})
    env["gpu_baseline"] = gm
    (OUT / "_env.json").write_text(json.dumps(env, indent=1), encoding="utf-8")
    try:
        for n in names:
            for mode in a.modes.split(","):
                be = backends[n]
                if mode == "equal4" and "concurrency" not in metrics:
                    continue
                if mode == "ctx65536" and "memory" not in metrics:
                    continue
                run_backend_mode(be, mode, metrics, base_mib)
            be.stop()
    finally:
        for b in backends.values():
            try:
                b.final_cleanup()
            except Exception as e:  # noqa: BLE001
                log("cleanup error", e)
    log("done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
