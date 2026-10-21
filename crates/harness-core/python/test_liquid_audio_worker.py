"""liquid_audio_worker's chat helpers (no torch, no model): request checking and the OpenAI
shaping of an answer, whole and as SSE. Argv: <liquid_audio_worker.py>. Prints PASS.
Run by `python_hf::tests::liquid_chat_helpers_shape_requests_and_answers` with any Python 3."""
import base64
import importlib.util
import json
import sys

spec = importlib.util.spec_from_file_location("liquid_audio_worker", sys.argv[1])
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)


def refused(body):
    try:
        w.chat_plan(body)
    except w.Failure as e:
        assert e.status == 400, e.status
        return str(e)
    raise AssertionError(f"{body} was accepted")


clip = {"type": "input_audio", "input_audio": {"data": "AAAA", "format": "wav"}}
text_only = w.chat_plan({"messages": [{"role": "user", "content": "Hello"}]})
assert text_only["speech"] is False and text_only["system"] == w.CHAT_SYSTEM_TEXT
assert text_only["turns"] == [("user", "Hello", [])] and text_only["max_tokens"] == w.CHAT_DEFAULT_TOKENS

plan = w.chat_plan({
    "messages": [{"role": "system", "content": "Be brief."},
                 {"role": "user", "content": [{"type": "text", "text": "Answer this"}, clip]}],
    "modalities": ["text", "audio"], "audio": {"voice": "alloy", "format": "wav"},
    "max_tokens": 5000, "temperature": 0.5, "stream": True,
})
assert plan["speech"] and plan["stream"] and plan["system"] == "Be brief."
assert plan["turns"] == [("user", "Answer this", ["AAAA"])]
assert plan["max_tokens"] == w.CHAT_MAX_TOKENS and plan["temperature"] == 0.5
# Speech without a system message asks for the interleaved text and audio answer.
spoken = w.chat_plan({"messages": [{"role": "user", "content": [clip]}], "modalities": ["audio"]})
assert spoken["system"] == w.CHAT_SYSTEM_SPEECH and spoken["turns"][0][1] == ""

assert "non-empty" in refused({"messages": []})
assert "modalities" in refused({"messages": [{"role": "user", "content": "x"}], "modalities": ["video"]})
assert "not served" in refused({"messages": [{"role": "user", "content": "x"}], "modalities": ["audio"],
                                "audio": {"format": "mp3"}})
assert "last message" in refused({"messages": [{"role": "assistant", "content": "x"}]})
assert "role" in refused({"messages": [{"role": "tool", "content": "x"}]})
assert "not supported" in refused({"messages": [{"role": "user", "content": [
    {"type": "image_url", "image_url": {"url": "data:,"}}]}]})
assert "base64" in refused({"messages": [{"role": "user", "content": [
    {"type": "input_audio", "input_audio": {"data": ""}}]}]})
assert "user messages" in refused({"messages": [
    {"role": "assistant", "content": [clip]}, {"role": "user", "content": "x"}]})
assert "at most" in refused({"messages": [{"role": "user", "content": [clip] * (w.MAX_CLIPS + 1)}]})
assert "max_tokens" in refused({"messages": [{"role": "user", "content": "x"}], "max_tokens": 0})
assert "temperature" in refused({"messages": [{"role": "user", "content": "x"}], "temperature": -1})

done = w.completion("lfm", "Hi there", b"RIFFxxxxWAVE", 7, 3, "stop")
message = done["choices"][0]["message"]
assert done["object"] == "chat.completion" and done["model"] == "lfm"
assert message["content"] == "Hi there" and message["audio"]["transcript"] == "Hi there"
assert base64.b64decode(message["audio"]["data"]) == b"RIFFxxxxWAVE"
assert done["usage"] == {"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10}
assert "audio" not in w.completion("lfm", "Hi", None, 1, 1, "length")["choices"][0]["message"]

events = w.completion_events(done)
assert events[-1] == b"data: [DONE]\n\n"
chunks = [json.loads(e[len(b"data: "):]) for e in events[:-1]]
assert chunks[0]["choices"][0]["delta"] == {"role": "assistant"}
assert chunks[1]["choices"][0]["delta"] == {"content": "Hi there"}
assert chunks[2]["choices"][0]["delta"]["audio"]["transcript"] == "Hi there"
assert chunks[-1]["choices"][0]["finish_reason"] == "stop" and chunks[-1]["usage"]["total_tokens"] == 10
assert all(c["object"] == "chat.completion.chunk" and c["id"] == done["id"] for c in chunks)

print("PASS")
