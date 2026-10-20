"""Small adapter for the documented upstream AIAgent API; never modifies upstream code."""
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, os.environ["HARNESS_HERMES_SOURCE"])
from run_agent import AIAgent

home = Path(os.environ["HERMES_HOME"])
request = json.loads((home / "harness-task.json").read_text(encoding="utf-8"))
model = request["model"]
agent = AIAgent(
    model=model["default"],
    provider="custom",
    base_url=model["base_url"],
    api_key=model["api_key"],
    enabled_toolsets=[],
    max_iterations=2,
    max_tokens=512,
    quiet_mode=True,
    skip_context_files=True,
    skip_memory=True,
    save_trajectories=False,
)
try:
    # Fail closed if an upstream change ever adds implicit tools to an empty allowlist.
    if agent.tools:
        raise RuntimeError("The upstream agent enabled tools despite an empty allowlist")
    result = agent.run_conversation(
        request["prompt"],
        system_message="You are Hermes Agent, connected through Harness Studio. Answer the user's task. No tools are enabled.",
    )
    print(result.get("final_response") or "", flush=True)
    if result.get("error"):
        print(str(result["error"]), file=sys.stderr, flush=True)
    if not result.get("completed"):
        raise SystemExit(1)
finally:
    agent.close()
