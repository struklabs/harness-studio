"""Remove everything the suite created: Harness bench-* definitions, Ollama bench-* models (and their blobs if
unreferenced), LM Studio hard links; unload all backends."""
import os
import shutil
import subprocess
from pathlib import Path

import setup_definitions
from backends import LMS, HOME, make_backends

env = dict(os.environ, OLLAMA_HOST="127.0.0.1:11437")
for b in make_backends().values():
    b.final_cleanup()
try:
    setup_definitions.remove()
except Exception as e:  # noqa: BLE001
    print("harness definitions:", e)
srv = subprocess.Popen(["ollama", "serve"], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
import time
time.sleep(4)
for m in ("bench-14b-ctx65536", "bench-14b-eq", "bench-4b-eq", "bench-14b", "bench-4b"):
    subprocess.run(["ollama", "rm", m], env=env)
srv.terminate()
subprocess.run([str(LMS), "unload", "--all"])
shutil.rmtree(HOME / ".lmstudio" / "models" / "bench", ignore_errors=True)
print("cleaned")
