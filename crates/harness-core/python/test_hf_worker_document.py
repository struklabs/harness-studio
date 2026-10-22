"""Document QA preprocessing and dispatch; no model downloads.

Run with the Python engine: python -m unittest discover -s crates/harness-core/python
-p test_hf_worker_document.py
"""
import base64
import io
import importlib.util
import sys
import unittest
from types import SimpleNamespace
from unittest.mock import Mock

from PIL import Image
if len(sys.argv) > 1 and sys.argv[1].endswith("hf_worker.py"):
    spec = importlib.util.spec_from_file_location("hf_worker", sys.argv[1])
    worker = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(worker)
else:
    import hf_worker as worker


class DocumentTests(unittest.TestCase):
    def test_polygons_become_normalized_word_boxes_in_reading_order(self):
        image = Image.new("RGB", (200, 100))
        ocr = Mock(return_value=([
            ([[20, 10], [180, 10], [180, 30], [20, 30]], "Total 42", .99),
            ([[-1, 80], [220, 80], [220, 110], [-1, 110]], "Paid", .9),
            ([[0, 0]] * 4, " ", .9),
        ], None))
        self.assertEqual(worker.document_word_boxes(ocr, image), [
            ("Total", [100, 100, 600, 300]),
            ("42", [700, 100, 900, 300]),
            ("Paid", [0, 800, 1000, 1000]),
        ])

    def test_dqa_dispatch_passes_ocr_words_and_preserves_score(self):
        image = Image.new("RGB", (200, 100), "white")
        png = io.BytesIO()
        image.save(png, format="PNG")
        data = "data:image/png;base64," + base64.b64encode(png.getvalue()).decode()
        model = SimpleNamespace(
            args=SimpleNamespace(task="document-question-answering"),
            pipe=Mock(return_value=[{"answer": "42", "score": .95}]),
            document_ocr=Mock(return_value=([
                ([[10, 10], [100, 10], [100, 30], [10, 30]], "Total 42", .99),
            ], None)),
        )
        result = worker.run_hf(model, {"inputs": {"image": data, "question": "What is the total?"}}, None)
        self.assertEqual(result, [{"answer": "42", "score": .95}])
        self.assertEqual([w for w, _ in model.pipe.call_args.kwargs["word_boxes"]], ["Total", "42"])
        self.assertEqual(model.pipe.call_args.kwargs["question"], "What is the total?")
        supplied = [("42", [0, 0, 100, 100])]
        model.document_ocr.reset_mock()
        worker.run_hf(model, {"inputs": {"image": data, "question": "Total?"},
                              "parameters": {"word_boxes": supplied}}, None)
        self.assertEqual(model.pipe.call_args.kwargs["word_boxes"], supplied)
        model.document_ocr.assert_not_called()
        model.document_ocr.return_value = (None, None)
        model.pipe.reset_mock()
        self.assertEqual(worker.run_hf(model, {"inputs": {"image": data, "question": "Total?"}}, None),
                         [{"answer": "", "score": 0.0}])
        model.pipe.assert_not_called()

    def test_generative_document_qa_does_not_require_ocr(self):
        model = SimpleNamespace(args=SimpleNamespace(task="document-question-answering"),
                                pipe=Mock(return_value=[{"answer": "42"}]))
        from unittest.mock import patch
        with patch.object(worker, "to_image", return_value=object()):
            self.assertEqual(worker.run_hf(model, {"inputs": {"image": "x", "question": "Total?"}}, None),
                             [{"answer": "42"}])
        self.assertNotIn("word_boxes", model.pipe.call_args.kwargs)


if __name__ == "__main__":
    result = unittest.main(argv=[sys.argv[0]], exit=False).result
    if not result.wasSuccessful():
        raise SystemExit(1)
    print("PASS")
