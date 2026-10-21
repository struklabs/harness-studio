"""Dependency-free regression checks for export asset preservation."""
import tempfile
import unittest
from pathlib import Path
from training_worker import export_tokenizer


class ExportTokenizerTests(unittest.TestCase):
    def test_preserves_original_assets_without_copying_weights_or_code(self):
        with tempfile.TemporaryDirectory() as temp:
            base, output = Path(temp) / 'base', Path(temp) / 'output'
            base.mkdir()
            assets = {
                'tokenizer_config.json': b'{"extra_special_tokens": {"image_token":"<image>"}}',
                'tokenizer.json': b'original tokenizer bytes',
                'tokenizer.model': b'sentencepiece bytes',
                'merges.txt': b'merges',
                'vocab.json': b'vocab',
                'special_tokens_map.json': b'special tokens',
                'chat_template.jinja': b'chat template',
            }
            for name, data in {**assets, 'model.safetensors': b'weights', 'config.json': b'base config', 'custom.py': b'code'}.items():
                (base / name).write_bytes(data)
            (base / 'chat_templates').mkdir()
            (base / 'chat_templates/default.jinja').write_bytes(b'default chat')
            (base / 'chat_templates/custom.py').write_bytes(b'code')
            export_tokenizer(base, output)
            for name, data in assets.items():
                self.assertEqual((output / name).read_bytes(), data)
            self.assertEqual((output / 'chat_templates/default.jinja').read_bytes(), b'default chat')
            for name in ['model.safetensors', 'config.json', 'custom.py', 'chat_templates/custom.py']:
                self.assertFalse((output / name).exists())


if __name__ == '__main__':
    unittest.main()
