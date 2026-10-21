"""Chat route dispatch without model downloads.

Run with the managed Python:
python -m unittest discover -s crates/harness-core/python -p test_hf_worker_routes.py
"""
import json
import threading
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import hf_worker as worker


class ChatRouteTests(unittest.IsolatedAsyncioTestCase):
    async def request(self, task, body):
        model = SimpleNamespace(args=SimpleNamespace(task=task), state="ok", lock=threading.Lock())
        with patch.dict(worker.os.environ, {"HARNESS_WORKER_API_KEY": "test-key"}):
            app = worker.build_app(model)
        messages = []

        async def receive():
            return {"type": "http.request", "body": json.dumps(body).encode(), "more_body": False}

        async def send(message):
            messages.append(message)

        await app({
            "type": "http", "asgi": {"version": "3.0", "spec_version": "2.4"},
            "method": "POST", "scheme": "http", "path": "/v1/chat/completions",
            "root_path": "", "query_string": b"",
            "headers": [(b"authorization", b"Bearer test-key"), (b"content-type", b"application/json")],
            "server": ("127.0.0.1", 12499), "client": ("127.0.0.1", 1),
        }, receive, send)
        status = next(m["status"] for m in messages if m["type"] == "http.response.start")
        data = b"".join(m.get("body", b"") for m in messages if m["type"] == "http.response.body")
        return status, data

    async def test_text_and_image_chat_use_chat_handler(self):
        for task in worker.CHAT_TASKS:
            with self.subTest(task=task), patch.object(worker, "run_chat", return_value={"answer": "chat"}) as chat, \
                    patch.object(worker, "video_chat") as video:
                status, data = await self.request(task, {"messages": []})
                self.assertEqual(status, 200)
                self.assertEqual(json.loads(data), {"answer": "chat"})
                chat.assert_called_once()
                video.assert_not_called()

    async def test_text_and_image_streaming_still_work(self):
        for task in worker.CHAT_TASKS:
            with self.subTest(task=task), patch.object(worker, "stream_chat", return_value=iter([b"data: [DONE]\n\n"])) as stream:
                status, data = await self.request(task, {"stream": True})
                self.assertEqual(status, 200)
                self.assertEqual(data, b"data: [DONE]\n\n")
                stream.assert_called_once()

    async def test_video_chat_uses_video_handler(self):
        with patch.object(worker, "video_chat", return_value={"answer": "video"}) as video, \
                patch.object(worker, "run_chat") as chat:
            status, data = await self.request("video-text-to-text", {"messages": []})
            self.assertEqual(status, 200)
            self.assertEqual(json.loads(data), {"answer": "video"})
            video.assert_called_once()
            chat.assert_not_called()

    async def test_video_streaming_is_rejected(self):
        with patch.object(worker, "video_chat") as video:
            status, _ = await self.request("video-text-to-text", {"stream": True})
            self.assertEqual(status, 400)
            video.assert_not_called()

    async def test_other_tasks_cannot_chat(self):
        with patch.object(worker, "run_chat") as chat:
            status, data = await self.request("text-classification", {})
            self.assertEqual(status, 404)
            self.assertEqual(json.loads(data)["error"]["code"], "wrong_task")
            chat.assert_not_called()


if __name__ == "__main__":
    unittest.main()
