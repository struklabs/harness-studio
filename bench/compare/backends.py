"""Backend adapters: each starts one backend (stopping the others first) and returns an Endpoint.

Models:  "4b"  = Qwen3-4B-Instruct-2507 Q4_K_M,  "14b" = Qwen2.5-Coder-14B-Instruct Q4_K_M
Modes:   out_of_box | equal | equal4 | ctx65536   (see README.md)
"""
from __future__ import annotations

import json
import os
import re
import subprocess
import time
from pathlib import Path

import httpx

from common import Endpoint, gpu_mem

# Other machines: BENCH_GGUF_4B / BENCH_GGUF_14B (the "14b" slot is the second model; on the 4 GB NVIDIA run it
# holds a small one, named by BENCH_SECOND in summarize.py).
GGUF = {
    "4b": os.environ.get("BENCH_GGUF_4B", r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf"),
    "14b": os.environ.get("BENCH_GGUF_14B", r"D:\Models\Qwen\Qwen2.5-Coder-14B-Instruct-GGUF\qwen2.5-coder-14b-instruct-q4_k_m.gguf"),
}
APPDATA = Path(os.environ.get("APPDATA", ""))
LOCALAPPDATA = Path(os.environ.get("LOCALAPPDATA", ""))
HOME = Path.home()
SCRATCH = Path(os.environ.get("BENCH_SCRATCH", str(LOCALAPPDATA / "Temp" / "bench-run")))
SCRATCH.mkdir(parents=True, exist_ok=True)

HARNESS_CLI = LOCALAPPDATA / "Harness Studio" / "harness.exe"
# HARNESS_DATA_DIR: a separate data root (the app started with the same variable), never the user's.
HARNESS_DATA = Path(os.environ["HARNESS_DATA_DIR"]) if os.environ.get("HARNESS_DATA_DIR") else APPDATA / "com.harnessstudio.app"
HARNESS_LLAMA = HARNESS_DATA / "bin" / "managed" / "llama-server.exe"
WINGET_LLAMA = Path(os.environ["BENCH_LLAMA_SERVER"]) if os.environ.get("BENCH_LLAMA_SERVER") else (
    LOCALAPPDATA / "Microsoft" / "WinGet" / "Packages" / "ggml.llamacpp_Microsoft.Winget.Source_8wekyb3d8bbwe" / "llama-server.exe")
LMS = HOME / ".lmstudio" / "bin" / "lms.exe"


def sh(args: list[str], timeout: float = 600, env: dict | None = None) -> tuple[int, str]:
    p = subprocess.run([str(a) for a in args], capture_output=True, text=True, encoding="utf-8", errors="replace",
                       timeout=timeout, env=env)
    return p.returncode, (p.stdout or "") + (p.stderr or "")


def ps_cmdlines(name: str) -> list[dict]:
    rc, out = sh(["powershell", "-NoProfile", "-Command",
                  f"Get-CimInstance Win32_Process -Filter \"Name='{name}'\" | "
                  "% { '{0}|{1}' -f $_.ProcessId,$_.CommandLine }"])
    res = []
    for line in out.splitlines():
        if "|" in line:
            pid, cmd = line.split("|", 1)
            res.append({"pid": int(pid), "cmd": cmd})
    return res


def wait_http(url: str, timeout: float, headers: dict | None = None, ok=(200,)) -> bool:
    end = time.time() + timeout
    while time.time() < end:
        try:
            if httpx.get(url, headers=headers, timeout=3).status_code in ok:
                return True
        except Exception:  # noqa: BLE001
            pass
        time.sleep(0.5)
    return False


class LoadError(Exception):
    pass


class Backend:
    name = "?"
    supports_on_demand_switch = False

    def stop(self) -> None: ...

    def serve(self, model: str, mode: str) -> Endpoint:
        raise NotImplementedError

    def version(self) -> str:
        return "?"

    def switch_to(self, model: str, mode: str) -> Endpoint | None:
        """Make `model` answerable via the API *without* the harness restarting anything. None = unsupported."""
        return None

    def final_cleanup(self) -> None:
        self.stop()


# ---------------------------------------------------------------------------
# Harness Studio (gateway on :11435, headless or GUI instance)
# ---------------------------------------------------------------------------

class Harness(Backend):
    name = "harness"
    supports_on_demand_switch = True
    GW = "http://127.0.0.1:11435/v1"
    API = "http://127.0.0.1:11500/api/harness/v1"
    ALIAS = {("4b", "out_of_box"): "bench-4b-default", ("4b", "equal"): "bench-4b-equal",
             ("4b", "equal4"): "bench-4b-equal4", ("14b", "out_of_box"): "bench-14b-default",
             ("14b", "equal"): "bench-14b-equal", ("14b", "ctx65536"): "bench-14b-ctx65536"}

    def _api_headers(self):
        tok = (HARNESS_DATA / "api-token").read_text().strip()
        return {"Authorization": f"Bearer {tok}"}

    def _gw_key(self):
        p = HARNESS_DATA / "gateway-token"
        return p.read_text().strip() if p.exists() else None

    def version(self) -> str:
        rc, out = sh([HARNESS_CLI, "--version"])
        rc2, rt = sh([HARNESS_LLAMA, "--version"])
        return f"{out.strip()} / managed runtime: {' '.join(rt.split()[:6])}"

    def stop(self) -> None:
        sh([HARNESS_CLI, "stop", "--all", "-y"])
        end = time.time() + 30
        while time.time() < end and ps_cmdlines("llama-server.exe"):
            time.sleep(1)

    def _info(self, alias: str) -> dict:
        info: dict = {"alias": alias}
        try:
            servers = httpx.get(self.API + "/servers", headers=self._api_headers(), timeout=10).json()
            for s in servers:
                if s.get("served_alias") == alias and s.get("status") == "Running":
                    eff = httpx.get(f"{self.API}/servers/{s['id']}/effective", headers=self._api_headers(),
                                    timeout=10).json()
                    info["server_id"] = s["id"]
                    info["config"] = {k: v for k, v in (eff.get("config") or {}).items() if v is not None
                                      and k in ("gpu_layers", "context_size", "flash_attn", "cache_type_k", "cache_type_v",
                                                "parallel_slots", "custom_args", "physical_batch", "logical_batch")}
                    info["values"] = {k: v for k, v in (eff.get("values") or {}).items() if k in (
                        "n_ctx", "total_slots", "n_ctx_train", "kv_unified", "gpu_layers_offloaded", "gpu_layers_total",
                        "ctx_adjusted", "gpu_layers_by_fit", "kv_fa_fallback", "fit_failed", "shared_memory_slowdown",
                        "shared_memory_bytes", "build_info", "chat_format")}
            for p in ps_cmdlines("llama-server.exe"):
                if alias in p["cmd"]:
                    info["cmdline"] = p["cmd"].split("llama-server.exe\"", 1)[-1].strip()
        except Exception as e:  # noqa: BLE001
            info["info_error"] = str(e)
        return info

    def _logs(self, alias: str) -> list[str]:
        d = HARNESS_DATA / "logs" / "servers" / f"gw-{alias}"
        if not d.exists():
            return []
        f = sorted(d.glob("*.log"))[-1]
        keep = re.compile(r"Harness Studio\]|n_ctx|offload|flash|kv|fit|memory", re.I)
        return [l.strip()[:300] for l in f.read_text(encoding="utf-8", errors="replace").splitlines() if keep.search(l)][:40]

    def _jit(self, alias: str) -> tuple[float, str | None]:
        ep = Endpoint(self.GW, alias, self._gw_key())
        t0 = time.perf_counter()
        r = httpx.post(self.GW + "/chat/completions", headers=ep.headers(), timeout=900,
                       json={"model": alias, "messages": [{"role": "user", "content": "Say OK."}], "max_tokens": 4,
                             "temperature": 0})
        dt = time.perf_counter() - t0
        return dt, (None if r.status_code == 200 else f"HTTP {r.status_code}: {r.text[:400]}")

    def serve(self, model: str, mode: str) -> Endpoint:
        alias = self.ALIAS[(model, mode)]
        self.stop()
        time.sleep(2)
        dt, err = self._jit(alias)
        info = self._info(alias)
        info["load_s"] = round(dt, 2)
        info["log_lines"] = self._logs(alias)
        if err:
            info["load_error"] = err
            raise LoadError(json.dumps(info)[:1500])
        return Endpoint(self.GW, alias, self._gw_key(), info)

    def switch_to(self, model: str, mode: str) -> Endpoint | None:
        alias = self.ALIAS[(model, mode)]
        return Endpoint(self.GW, alias, self._gw_key(), {"alias": alias})


# ---------------------------------------------------------------------------
# LM Studio (lms server on :1234)
# ---------------------------------------------------------------------------

class LMStudio(Backend):
    name = "lmstudio"
    supports_on_demand_switch = True  # only if JIT loading is active; probed at run time
    BASE = "http://127.0.0.1:1234/v1"
    KEY = {"4b": "qwen3-4b-instruct-2507", "14b": "qwen2.5-coder-14b-instruct"}

    def version(self) -> str:
        rc, out = sh([LMS, "version"])
        eng = sh([LMS, "runtime", "ls"])[1]
        return (re.sub(r"\x1b\[[0-9;]*m", "", out).strip().splitlines()[-1:] or [""])[0] + " | " + " ".join(
            re.sub(r"\x1b\[[0-9;]*m", "", eng).split())[:300]

    def _ensure_server(self):
        try:
            if httpx.get(self.BASE + "/models", timeout=3).status_code == 200:
                return
        except Exception:  # noqa: BLE001
            pass
        sh([LMS, "server", "start", "--port", "1234"], timeout=60)
        if not wait_http(self.BASE + "/models", 40):
            raise LoadError("lms server did not start")

    def stop(self) -> None:
        sh([LMS, "unload", "--all"], timeout=60)
        time.sleep(2)

    def _ps(self) -> list[dict]:
        rc, out = sh([LMS, "ps", "--json"])
        try:
            return json.loads(out[out.index("["):])
        except Exception:  # noqa: BLE001
            return []

    def _rest_models(self) -> dict:
        try:
            return httpx.get("http://127.0.0.1:1234/api/v1/models", timeout=10).json()
        except Exception as e:  # noqa: BLE001
            return {"error": str(e)}

    def serve(self, model: str, mode: str) -> Endpoint:
        self._ensure_server()
        self.stop()
        flags: list[str] = []
        if mode == "equal":
            flags = ["--context-length", "32768", "--gpu", "max", "--parallel", "1"]
        elif mode == "equal4":
            flags = ["--context-length", "32768", "--gpu", "max", "--parallel", "4"]
        elif mode == "ctx65536":
            flags = ["--context-length", "65536"]
        key = self.KEY[model]
        t0 = time.perf_counter()
        rc, out = sh([LMS, "load", key, "--identifier", key, "-y", *flags], timeout=900)
        dt = time.perf_counter() - t0
        info: dict = {"load_s": round(dt, 2), "flags": flags, "lms_load_tail": re.sub(r"\x1b\[[0-9;?]*[a-zA-Z]", "", out)[-300:].replace("\r", " ")}
        if rc != 0:
            info["load_error"] = info["lms_load_tail"]
            raise LoadError(json.dumps(info)[:1500])
        ps = self._ps()
        info["ps"] = [{k: v for k, v in p.items() if k in ("identifier", "contextLength", "parallel", "status", "sizeBytes")} for p in ps]
        rest = self._rest_models()
        for m in rest.get("models", []) if isinstance(rest, dict) else []:
            for inst in m.get("loaded_instances", []):
                if inst.get("id") == key:
                    info["rest_config"] = inst.get("config")
        info["log_lines"] = self._engine_log_lines()
        return Endpoint(self.BASE, key, None, info)

    def _engine_log_lines(self) -> list[str]:
        logs = sorted((HOME / ".lmstudio" / "server-logs").glob("*/*.log"))
        if not logs:
            return []
        keep = re.compile(r"n_ctx|n_slots|offload|flash|kv_|layers|fit|cache_type|Just-in-time", re.I)
        return [l.strip()[:260] for l in logs[-1].read_text(encoding="utf-8", errors="replace").splitlines()
                if keep.search(l)][-12:]

    def jit_active(self) -> bool:
        logs = sorted((HOME / ".lmstudio" / "server-logs").glob("*/*.log"))
        txt = logs[-1].read_text(encoding="utf-8", errors="replace") if logs else ""
        hits = re.findall(r"Just-in-time model loading (\w+)", txt)
        return bool(hits) and hits[-1] == "active"

    def switch_to(self, model: str, mode: str) -> Endpoint | None:
        # LM Studio can only serve a model it has loaded, unless JIT loading is active in the app.
        return Endpoint(self.BASE, self.KEY[model], None, {"jit_active": self.jit_active()})


# ---------------------------------------------------------------------------
# Ollama (own `ollama serve` on :11437 so the user's instance and its env vars are untouched)
# ---------------------------------------------------------------------------

class Ollama(Backend):
    name = "ollama"
    supports_on_demand_switch = True
    PORT = 11437
    BASE = f"http://127.0.0.1:{PORT}"
    NAMES = {("4b", "out_of_box"): "bench-4b", ("4b", "equal"): "bench-4b-eq", ("4b", "equal4"): "bench-4b-eq",
             ("14b", "out_of_box"): "bench-14b", ("14b", "equal"): "bench-14b-eq", ("14b", "equal4"): "bench-14b-eq",
             ("14b", "ctx65536"): "bench-14b-ctx65536"}
    ENVS = {"out_of_box": {}, "ctx65536": {}, "equal": {"OLLAMA_FLASH_ATTENTION": "1", "OLLAMA_KV_CACHE_TYPE": "f16", "OLLAMA_NUM_PARALLEL": "1"},
            "equal4": {"OLLAMA_FLASH_ATTENTION": "1", "OLLAMA_KV_CACHE_TYPE": "f16", "OLLAMA_NUM_PARALLEL": "4"}}
    STRIP = ("OLLAMA_FLASH_ATTENTION", "OLLAMA_KV_CACHE_TYPE", "OLLAMA_NUM_PARALLEL", "OLLAMA_MAX_LOADED_MODELS",
             "OLLAMA_CONTEXT_LENGTH", "OLLAMA_NUM_CTX", "OLLAMA_GPU_OVERHEAD")

    def __init__(self):
        self.proc: subprocess.Popen | None = None
        self.env_key: str | None = None
        self.log = SCRATCH / "ollama-serve.log"

    def version(self) -> str:
        return sh(["ollama", "--version"])[1].strip().splitlines()[-1]

    def _start(self, env_key: str):
        if self.proc and self.proc.poll() is None and self.env_key == env_key:
            return
        self._kill()
        env = {k: v for k, v in os.environ.items() if k not in self.STRIP}
        env.update(self.ENVS[env_key])
        env["OLLAMA_HOST"] = f"127.0.0.1:{self.PORT}"
        env["OLLAMA_DEBUG"] = "1"
        self.proc = subprocess.Popen(["ollama", "serve"], env=env, stdout=open(self.log, "w"), stderr=subprocess.STDOUT)
        self.env_key = env_key
        if not wait_http(self.BASE + "/api/version", 40):
            raise LoadError("ollama serve did not start")

    def _kill(self):
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(15)
            except Exception:  # noqa: BLE001
                self.proc.kill()
        # also stop the runner children of OUR server only (they die with the parent)
        self.proc = None
        time.sleep(2)

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            try:
                for m in httpx.get(self.BASE + "/api/ps", timeout=5).json().get("models", []):
                    httpx.post(self.BASE + "/api/generate", json={"model": m["name"], "keep_alive": 0}, timeout=60)
            except Exception:  # noqa: BLE001
                pass
            time.sleep(2)

    def final_cleanup(self) -> None:
        self.stop()
        self._kill()

    def serve(self, model: str, mode: str) -> Endpoint:
        self._start(mode if mode in self.ENVS else "out_of_box")
        self.stop()
        name = self.NAMES[(model, mode)]
        ep = Endpoint(self.BASE + "/v1", name, None, {"env": self.ENVS[mode], "server_env_stripped": list(self.STRIP)})
        t0 = time.perf_counter()
        r = httpx.post(self.BASE + "/v1/chat/completions", timeout=900,
                       json={"model": name, "messages": [{"role": "user", "content": "Say OK."}], "max_tokens": 4, "temperature": 0})
        ep.info["load_s"] = round(time.perf_counter() - t0, 2)
        if r.status_code != 200:
            ep.info["load_error"] = f"HTTP {r.status_code}: {r.text[:400]}"
            raise LoadError(json.dumps(ep.info)[:1500])
        ep.info.update(self._ps_info(name))
        return ep

    def _ps_info(self, name: str) -> dict:
        info: dict = {}
        try:
            for m in httpx.get(self.BASE + "/api/ps", timeout=10).json().get("models", []):
                if m["name"].startswith(name):
                    info["ps"] = {k: m.get(k) for k in ("size", "size_vram", "context_length")}
        except Exception as e:  # noqa: BLE001
            info["ps_error"] = str(e)
        try:
            txt = self.log.read_text(encoding="utf-8", errors="replace").splitlines()
            keep = re.compile(r"flash|KvCacheType|NumCtx|NumParallel|offload|layers|num_gpu|FlashAttention|gpu_layers|runner started|KV cache", re.I)
            info["log_lines"] = [re.sub(r"\s+", " ", l)[:260] for l in txt if keep.search(l)][-14:]
        except Exception:  # noqa: BLE001
            pass
        return info

    def switch_to(self, model: str, mode: str) -> Endpoint | None:
        return Endpoint(self.BASE + "/v1", self.NAMES[(model, mode)], None, {})

    def refresh_info(self, ep: Endpoint) -> dict:
        return self._ps_info(ep.model)


# ---------------------------------------------------------------------------
# Plain llama-server
# ---------------------------------------------------------------------------

class LlamaServer(Backend):
    supports_on_demand_switch = False

    def __init__(self, name: str, exe: Path, port: int):
        self.name = name
        self.exe = exe
        self.port = port
        self.proc: subprocess.Popen | None = None
        self.log = SCRATCH / f"{name}.log"

    def version(self) -> str:
        return " ".join(sh([self.exe, "--version"])[1].split()[:8])

    def stop(self) -> None:
        if self.proc and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(20)
            except Exception:  # noqa: BLE001
                self.proc.kill()
        self.proc = None
        time.sleep(2)

    def args(self, model: str, mode: str) -> list[str]:
        a = ["-m", GGUF[model], "--host", "127.0.0.1", "--port", str(self.port)]
        if mode in ("equal", "equal4"):
            a += ["-c", "32768", "-ngl", "999", "-fa", "on", "-ctk", "f16", "-ctv", "f16", "-np", "4" if mode == "equal4" else "1"]
        elif mode == "ctx65536":
            a += ["-c", "65536"]
        return a

    PROBE_KEEP = re.compile(r"offloaded|flash|KV buffer|KV self|n_ctx_seq|n_ctx\b|n_seq_max|type_k|type_v|fit|n_gpu_layers|n_parallel", re.I)

    def probe(self, args: list[str]) -> list[str]:
        """Start once more at higher log verbosity (same arguments) only to record layers / FA / KV type; then stop."""
        logf = SCRATCH / f"{self.name}-probe.log"
        p = subprocess.Popen([str(self.exe), *args, "-lv", "4"], stdout=open(logf, "w"), stderr=subprocess.STDOUT)
        end = time.time() + 600
        while time.time() < end and p.poll() is None:
            try:
                if httpx.get(f"http://127.0.0.1:{self.port}/health", timeout=2).status_code == 200:
                    break
            except Exception:  # noqa: BLE001
                pass
            time.sleep(0.5)
        p.terminate()
        try:
            p.wait(20)
        except Exception:  # noqa: BLE001
            p.kill()
        time.sleep(2)
        seen: list[str] = []
        for l in logf.read_text(encoding="utf-8", errors="replace").splitlines():
            if self.PROBE_KEEP.search(l):
                t = re.sub(r"\s+", " ", re.sub(r"^[0-9.]+ [A-Z] ", "", l))[:200]
                if t not in seen:
                    seen.append(t)
        return seen[:25]

    def serve(self, model: str, mode: str) -> Endpoint:
        self.stop()
        args = self.args(model, mode)
        probe_lines = self.probe(args)
        f = open(self.log, "w")
        t0 = time.perf_counter()
        self.proc = subprocess.Popen([str(self.exe), *args], stdout=f, stderr=subprocess.STDOUT)
        ok = False
        end = time.time() + 600
        while time.time() < end and self.proc.poll() is None:
            try:
                if httpx.get(f"http://127.0.0.1:{self.port}/health", timeout=2).status_code == 200:
                    ok = True
                    break
            except Exception:  # noqa: BLE001
                pass
            time.sleep(0.5)
        dt = time.perf_counter() - t0
        info: dict = {"args": args, "load_s": round(dt, 2), "probe_lines": probe_lines}
        txt = self.log.read_text(encoding="utf-8", errors="replace")
        keep = re.compile(r"n_ctx|n_slots|offload|flash|fit|kv_unified|n_gpu_layers|KV buffer|cache_type|failed|error|out of memory", re.I)
        info["log_lines"] = [re.sub(r"\s+", " ", l)[:260] for l in txt.splitlines() if keep.search(l)][:30]
        if not ok:
            info["load_error"] = "server exited or timed out; tail: " + txt[-600:]
            raise LoadError(json.dumps(info)[:2000])
        try:
            props = httpx.get(f"http://127.0.0.1:{self.port}/props", timeout=10).json()
            dgs = props.get("default_generation_settings", {})
            info["props"] = {"total_slots": props.get("total_slots"), "n_ctx": dgs.get("n_ctx"),
                             "build_info": props.get("build_info")}
        except Exception as e:  # noqa: BLE001
            info["props_error"] = str(e)
        return Endpoint(f"http://127.0.0.1:{self.port}/v1", "bench", None, info)

    def switch_to(self, model: str, mode: str) -> Endpoint | None:
        return None  # a plain llama-server serves exactly one model; switching = kill + start


def make_backends() -> dict[str, Backend]:
    return {
        "harness": Harness(),
        "lmstudio": LMStudio(),
        "ollama": Ollama(),
        "llamacpp": LlamaServer("llamacpp", WINGET_LLAMA, 8090),
        "llamacpp_b11552": LlamaServer("llamacpp_b11552", HARNESS_LLAMA, 8091),
    }


def stop_all(backends: dict[str, Backend], except_name: str | None = None) -> None:
    for n, b in backends.items():
        if n != except_name:
            b.stop()


def baseline_ok(baseline_mib: int, slack: int = 400, wait: float = 40) -> dict:
    end = time.time() + wait
    m = gpu_mem()
    while time.time() < end:
        m = gpu_mem()
        if m.get("dedicated_mib", 0) <= baseline_mib + slack:
            break
        time.sleep(3)
    return m
