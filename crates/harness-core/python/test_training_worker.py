"""Dependency-free regression checks for export asset preservation."""
import tempfile
import unittest
import contextlib
import io
import json
import types
from unittest.mock import patch
from pathlib import Path
from training_worker import export_tokenizer
from training_worker import compare, generate_samples


class GenerationTests(unittest.TestCase):
    def test_checkpoint_samples_decode_only_answer_and_use_greedy_bounded_generation(self):
        calls = []
        class Batch(dict):
            def to(self, device):
                self.device = device
                return self
        class Tokenizer:
            pad_token_id = 0
            def apply_chat_template(self, messages, **kwargs):
                calls.append((messages, kwargs))
                return 'chat'
            def __call__(self, text, **kwargs):
                self.options = kwargs
                return Batch(input_ids=types.SimpleNamespace(shape=(1, 2)))
            def decode(self, tokens, **kwargs):
                self.decoded = tokens
                return 'answer'
        tokenizer = Tokenizer()
        model = types.SimpleNamespace(training=True, device='cpu')
        model.eval = lambda: setattr(model, 'training', False)
        model.train = lambda value: setattr(model, 'training', value)
        def generate(**kwargs):
            self.assertFalse(model.training)
            self.assertFalse(kwargs['do_sample'])
            self.assertEqual(kwargs['max_new_tokens'], 128)
            return [[1, 2, 9]]
        model.generate = generate
        with patch.dict('sys.modules', {'torch': types.SimpleNamespace(inference_mode=contextlib.nullcontext)}):
            samples = generate_samples(model, tokenizer, ['Hello'], 128)
        self.assertEqual(samples, [{'prompt': 'Hello', 'text': 'answer'}])
        self.assertEqual(tokenizer.decoded, [9])
        self.assertEqual(tokenizer.options['max_length'], 128)
        self.assertEqual(calls[0][0], [{'role': 'user', 'content': 'Hello'}])
        self.assertTrue(calls[0][1]['add_generation_prompt'])
        self.assertTrue(model.training)
        model.config = types.SimpleNamespace(max_position_embeddings=128)
        def bounded_generate(**kwargs):
            self.assertEqual(kwargs['max_new_tokens'], 64)
            return [[1, 2, 9]]
        model.generate = bounded_generate
        with patch.dict('sys.modules', {'torch': types.SimpleNamespace(inference_mode=contextlib.nullcontext)}):
            generate_samples(model, tokenizer, ['Hello'], 128)
        self.assertEqual(tokenizer.options['max_length'], 64)
        self.assertTrue(model.training)

    def test_comparison_uses_same_prompts_and_limits_on_base_then_adapter(self):
        base, trained = object(), object()
        calls = []
        tokenizer = types.SimpleNamespace(pad_token_id=0)
        def generate(model, tok, prompts, limit):
            calls.append((model, tok, list(prompts), limit))
            return [{"prompt": p, "text": ("before" if model is base else "after") + p} for p in prompts]
        modules = {
            'torch': types.SimpleNamespace(float32='f32'),
            'peft': types.SimpleNamespace(PeftModel=types.SimpleNamespace(from_pretrained=lambda *a, **k: trained)),
            'transformers': types.SimpleNamespace(
                AutoModelForCausalLM=types.SimpleNamespace(from_pretrained=lambda *a, **k: base),
                AutoTokenizer=types.SimpleNamespace(from_pretrained=lambda *a, **k: tokenizer)),
        }
        with tempfile.TemporaryDirectory() as temp, patch.dict('sys.modules', modules), \
                patch('training_worker.generate_samples', side_effect=generate):
            root = Path(temp)
            (root / 'prompts.json').write_text(json.dumps(['A', 'B', 'A']), encoding='utf-8')
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                compare(root, {'model': '/local/model', 'sequence_length': 128})
            rows = json.loads(output.getvalue().removeprefix('HS_COMPARE '))
        self.assertEqual([c[0] for c in calls], [base, trained])
        self.assertEqual(calls[0][1:], calls[1][1:])
        self.assertEqual([r['prompt'] for r in rows], ['A', 'B', 'A'])
        self.assertEqual(rows[1], {'prompt': 'B', 'base': 'beforeB', 'trained': 'afterB'})

    def test_checkpoint_generation_restores_training_mode_on_failure(self):
        model = types.SimpleNamespace(training=True)
        model.eval = lambda: setattr(model, 'training', False)
        model.train = lambda value: setattr(model, 'training', value)
        tokenizer = types.SimpleNamespace(apply_chat_template=lambda *a, **k: (_ for _ in ()).throw(ValueError('template')))
        with patch.dict('sys.modules', {'torch': types.SimpleNamespace()}):
            with self.assertRaisesRegex(ValueError, 'template'):
                generate_samples(model, tokenizer, ['Hello'])
        self.assertTrue(model.training)


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
