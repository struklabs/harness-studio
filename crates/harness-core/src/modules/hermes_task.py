"""Small adapter for the documented upstream AIAgent API; never modifies upstream code."""
import json
import os
from pathlib import Path
import sys
import time

sys.path.insert(0, os.environ["HARNESS_HERMES_SOURCE"])
from run_agent import AIAgent

home = Path(os.environ["HERMES_HOME"])
request = json.loads((home / "harness-task.json").read_text(encoding="utf-8"))
model = request["model"]
def canonical(path):
    resolved = path.resolve(strict=True)
    # Rust canonicalize uses extended local paths on Windows. Compare those
    # with ordinary drive paths using the same representation.
    return Path(str(resolved).removeprefix("\\\\?\\")) if os.name == "nt" else resolved

folders = [canonical(Path(p)) for p in request.get("read_folders", [])]

def event(kind, **data):
    print("HARNESS_EVENT:" + json.dumps({"kind": kind, "at": int(time.time() * 1000), **data}), flush=True)

def read_file(args, **_kwargs):
    # Resolve junctions/symlinks before checking containment; never expose writes,
    # shell or network tools. Upstream remains trusted code, not an OS sandbox.
    try:
        path = Path(args["path"])
        if os.name == "nt":
            # Reject UNC/device paths and reparse points before resolving: even
            # resolving an ungranted network path can initiate network I/O.
            drive = path.drive.removeprefix("\\\\?\\")
            if not path.is_absolute() or len(drive) != 2 or not drive[0].isalpha() or drive[1] != ":":
                raise PermissionError("Only absolute local file paths are allowed")
            for part in reversed([path, *path.parents]):
                if getattr(part.lstat(), "st_file_attributes", 0) & 0x400:
                    raise PermissionError("Reparse points are not allowed")
        path = canonical(path)
        if not any(path.is_relative_to(folder) for folder in folders):
            raise PermissionError("Outside the granted folders")
        if not path.is_file():
            raise PermissionError("Not a regular file")
        with path.open("rb") as source:
            content = source.read(32769)
        if len(content) > 32768:
            raise PermissionError("File exceeds 32 KiB")
        event("tool", name="harness_read_file", path=str(path), status="completed")
        return json.dumps({"content": content.decode("utf-8", errors="replace")})
    except (OSError, ValueError, KeyError, TypeError) as error:
        event("tool", name="harness_read_file", status="denied", error=str(error))
        return json.dumps({"error": str(error)})

if folders:
    from tools.registry import registry
    registry.register(
        name="harness_read_file", toolset="harness_read",
        schema={"name": "harness_read_file", "description": "Read a UTF-8 file (maximum 32 KiB) inside the user-granted folders: " + ", ".join(map(str, folders)),
                "parameters": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"], "additionalProperties": False}},
        handler=read_file,
    )
agent = AIAgent(
    model=model["default"],
    provider="custom",
    base_url=model["base_url"],
    api_key=model["api_key"],
    enabled_toolsets=["harness_read"] if folders else [],
    max_iterations=4,
    max_tokens=512,
    quiet_mode=True,
    skip_context_files=True,
    skip_memory=True,
    save_trajectories=False,
)
try:
    # Fail closed if an upstream change ever adds implicit tools to an empty allowlist.
    allowed = {"harness_read_file"} if folders else set()
    actual = {tool["function"]["name"] for tool in agent.tools}
    if actual != allowed:
        raise RuntimeError("The upstream agent did not honor the exact tool allowlist")
    result = agent.run_conversation(
        request["prompt"],
        system_message="You are Hermes Agent, connected through Harness Studio. Answer the user's task using only the explicitly available tools.",
    )
    event("output", text=result.get("final_response") or "")
    event("usage", tokens=getattr(agent, "session_total_tokens", None))
    if result.get("error"):
        print(str(result["error"]), file=sys.stderr, flush=True)
    if not result.get("completed"):
        raise SystemExit(1)
finally:
    agent.close()
