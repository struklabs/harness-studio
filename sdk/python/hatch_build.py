"""Generates ``harness_studio_api`` from ``sdk/openapi.json`` before a build when it is missing.

The generated package is not committed. Building from a checkout runs ``sdk/generate.mjs``
(needs Node.js and uv); an sdist already contains the generated code.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

from hatchling.builders.hooks.plugin.interface import BuildHookInterface


def ensure_generated(root: Path) -> None:
    if (root / "src" / "harness_studio_api" / "__init__.py").exists():
        return
    script = root.parent / "generate.mjs"
    if not script.exists():
        raise RuntimeError("src/harness_studio_api is missing and sdk/generate.mjs is not here")
    subprocess.run(
        ["node", str(script), "--from-snapshot", "--skip-typescript"], check=True, cwd=root
    )


class GenerateHook(BuildHookInterface):
    PLUGIN_NAME = "custom"

    def initialize(self, version: str, build_data: dict) -> None:
        ensure_generated(Path(self.root))
