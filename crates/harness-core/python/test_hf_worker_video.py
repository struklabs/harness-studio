"""Bounded video input, real MP4/WebM encoding, and diffusion callback cancellation.
Run with the managed Python: python -m unittest discover -s crates/harness-core/python -p test_hf_worker_video.py.
"""
import base64
import io
import threading
import unittest
from types import SimpleNamespace

import av
from PIL import Image
import hf_worker as worker


class Pipeline:
    def __init__(self):
        self.called = False

    def __call__(self, prompt, num_frames, callback_on_step_end, **kwargs):
        self.called = True
        callback_on_step_end(self, 0, 0, {})
        return SimpleNamespace(frames=[[Image.new("RGB", (64, 64), "red") for _ in range(num_frames)]])


class VideoTests(unittest.TestCase):
    def model(self):
        return SimpleNamespace(pipe=Pipeline(), lock=threading.Lock(),
                               args=SimpleNamespace(task="text-to-video"), max_pixels=lambda: 1_000_000)

    def body(self, fmt):
        return {"prompt": "red", "video_frames": 9, "width": 64, "height": 64, "output_format": fmt,
                "sample_params": {"sample_steps": 1}}

    def test_real_containers_and_bounded_frame_sampling(self):
        for fmt in ("mp4", "webm"):
            job = {"cancel": threading.Event()}
            output = worker.generate_video(self.model(), self.body(fmt), job)
            data = base64.b64decode(output)
            with av.open(io.BytesIO(data)) as container:
                self.assertEqual(len(list(container.decode(video=0))), 9)
            self.assertEqual(len(worker.video_frames("data:video/"+fmt+";base64,"+output, count=3)), 3)
            self.assertEqual(job["preview"], {"step": 1, "total_steps": 1})

    def test_cancelled_queued_generation_never_calls_pipeline(self):
        model = self.model()
        cancel = threading.Event(); cancel.set()
        with self.assertRaises(worker.GenerationCancelled):
            worker.generate_video(model, self.body("mp4"), {"cancel": cancel})
        self.assertFalse(model.pipe.called)

    def test_model_requiring_start_image_rejects_text_only(self):
        model = self.model(); model.args.task = "image-to-video"
        with self.assertRaises(worker.BadRequest):
            worker.generate_video(model, self.body("mp4"), {"cancel": threading.Event()})

    def test_video_chat_does_not_fetch_urls_or_read_paths(self):
        for source in ("https://example.com/clip.mp4", "file:///tmp/clip.mp4", "/tmp/clip.mp4"):
            with self.assertRaises(worker.BadRequest):
                worker.video_chat(self.model(), {"messages": [{"role": "user", "content": [
                    {"type": "video_url", "video_url": {"url": source}}]}]})


if __name__ == "__main__":
    unittest.main()
