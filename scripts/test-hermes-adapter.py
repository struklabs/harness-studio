"""Exercise the shipped adapter with a deterministic upstream API double."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ADAPTER = Path(__file__).resolve().parents[1] / "crates/harness-core/src/modules/hermes_task.py"
RUNTIME = '''
import json
from tools.registry import registry
class AIAgent:
    def __init__(self, **kwargs):
        assert kwargs['enabled_toolsets'] in ([], ['harness_read'])
        assert kwargs['skip_memory'] and kwargs['skip_context_files']
        self.tools = [{'function': {'name': name}} for name in registry.handlers]
        self.session_total_tokens = 17
    def run_conversation(self, prompt, **kwargs):
        results = [json.loads(registry.handlers['harness_read_file']({'path': p})) for p in json.loads(prompt)]
        return {'completed': True, 'final_response': json.dumps(results)}
    def close(self):
        pass
'''


class AdapterTests(unittest.TestCase):
    def run_adapter(self, root, folders, paths, implicit=False):
        source = root / "source"
        source.mkdir(exist_ok=True)
        (source / "run_agent.py").write_text(RUNTIME, encoding="utf-8")
        tools = source / "tools"
        tools.mkdir(exist_ok=True)
        (tools / "__init__.py").write_text("", encoding="utf-8")
        (tools / "registry.py").write_text(
            "class Registry:\n"
            "    def __init__(self): self.handlers = {}\n"
            "    def register(self, name, handler, **kwargs): self.handlers[name] = handler\n"
            "registry = Registry()\n" +
            ("registry.handlers['unexpected_shell'] = None\n" if implicit else ""),
            encoding="utf-8",
        )
        home = root / "home"
        home.mkdir(exist_ok=True)
        (home / "harness-task.json").write_text(json.dumps({
            "model": {"default": "chat", "base_url": "http://localhost/v1", "api_key": "test"},
            "read_folders": list(map(str, folders)), "prompt": json.dumps(list(map(str, paths))),
        }), encoding="utf-8")
        env = {**os.environ, "HARNESS_HERMES_SOURCE": str(source), "HERMES_HOME": str(home)}
        return subprocess.run([sys.executable, str(ADAPTER)], env=env, capture_output=True, text=True, timeout=10)

    def test_default_none_and_implicit_tools_fail_closed(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            result = self.run_adapter(root, [], [])
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('"tokens": 17', result.stdout)
            result = self.run_adapter(root, [], [], implicit=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("exact tool allowlist", result.stderr)

    def test_file_boundaries_and_audit(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            allowed = root / "allowed"
            allowed.mkdir()
            good = allowed / "good.txt"
            good.write_text("hello", encoding="utf-8")
            outside = root / "secret.txt"
            outside.write_text("never disclose", encoding="utf-8")
            large = allowed / "large.txt"
            large.write_bytes(b"x" * 32769)
            paths = [good, allowed / ".." / "secret.txt", allowed, large]
            link = allowed / "link.txt"
            try:
                link.symlink_to(outside)
                paths.append(link)
            except OSError:
                pass  # Windows may require Developer Mode for symlinks.
            scope = Path("\\\\?\\" + str(allowed)) if os.name == "nt" else allowed
            result = self.run_adapter(root, [scope], paths)
            self.assertEqual(result.returncode, 0, result.stderr)
            events = [json.loads(line.removeprefix("HARNESS_EVENT:")) for line in result.stdout.splitlines()]
            output = next(e for e in events if e["kind"] == "output")
            answers = json.loads(output["text"])
            self.assertEqual(answers[0], {"content": "hello"})
            self.assertTrue(all("error" in answer for answer in answers[1:]))
            self.assertNotIn("never disclose", result.stdout)
            self.assertEqual(len([e for e in events if e["kind"] == "tool"]), len(paths))


if __name__ == "__main__":
    unittest.main()
