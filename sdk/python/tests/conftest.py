"""The tests run against the client generated from the committed ``sdk/openapi.json``."""

import subprocess
from pathlib import Path

SDK = Path(__file__).resolve().parents[2]


def pytest_sessionstart(session):
    subprocess.run(
        ["node", str(SDK / "generate.mjs"), "--from-snapshot", "--skip-typescript"],
        check=True,
        cwd=SDK,
    )
