"""A request's LoRA file must be inside the Harness model folders (`HARNESS_LORA_ROOTS`).

Runs without model downloads:
python -m unittest discover -s crates/harness-core/python -p test_hf_worker_lora_paths.py
"""
import os
import tempfile
import unittest
from unittest.mock import patch

import hf_worker as worker


class LoraPathTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = os.path.realpath(self.tmp.name)
        self.models = os.path.join(base, "models")
        self.outside = os.path.join(base, "outside")
        os.makedirs(self.models)
        os.makedirs(self.outside)
        self.inside_file = os.path.join(self.models, "style.safetensors")
        self.outside_file = os.path.join(self.outside, "secret.safetensors")
        for p in (self.inside_file, self.outside_file):
            with open(p, "wb") as f:
                f.write(b"x")
        self.env = patch.dict(os.environ, {"HARNESS_LORA_ROOTS": self.models})
        self.env.start()

    def tearDown(self):
        self.env.stop()
        self.tmp.cleanup()

    def refused(self, path):
        with self.assertRaises(worker.BadRequest) as e:
            worker.lora_request({"lora": path})
        return str(e.exception)

    def test_a_file_inside_the_model_folders_is_accepted(self):
        spec = worker.lora_request({"lora": self.inside_file})
        self.assertEqual(spec[0], "file")
        self.assertEqual(os.path.normcase(spec[1]), os.path.normcase(self.inside_file))

    def test_an_outside_file_is_refused_like_a_missing_one(self):
        outside = self.refused(self.outside_file)
        missing = self.refused(os.path.join(self.outside, "nope.safetensors"))
        self.assertEqual(outside, missing)
        self.assertIn("not available", outside)
        dotdot = self.refused(os.path.join(self.models, "..", "outside", "secret.safetensors"))
        self.assertEqual(dotdot, outside)

    def test_a_symlink_escaping_the_model_folders_is_refused(self):
        link = os.path.join(self.models, "escape.safetensors")
        try:
            os.symlink(self.outside_file, link)
        except (OSError, NotImplementedError):
            self.skipTest("symlinks are not available here")
        self.assertIn("not available", self.refused(link))

    def test_a_junction_escaping_the_model_folders_is_refused(self):
        # Windows: a directory junction needs no privilege, unlike a symlink.
        if os.name != "nt":
            self.skipTest("junctions are Windows only")
        import subprocess
        junction = os.path.join(self.models, "linked")
        made = subprocess.run(
            ["cmd", "/c", "mklink", "/J", junction, self.outside],
            capture_output=True,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        if made.returncode != 0:
            self.skipTest("could not create a junction")
        try:
            self.assertIn("not available", self.refused(os.path.join(junction, "secret.safetensors")))
        finally:
            os.rmdir(junction)

    def test_no_roots_means_no_local_file(self):
        with patch.dict(os.environ, {"HARNESS_LORA_ROOTS": ""}):
            self.assertIn("not available", self.refused(self.inside_file))

    def test_repository_ids_are_unchanged(self):
        self.assertEqual(worker.lora_request({"lora": "owner/name"}), ("repo", "owner/name", None, 1.0))


if __name__ == "__main__":
    unittest.main()
