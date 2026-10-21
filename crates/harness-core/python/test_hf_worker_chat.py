"""hf_worker's chat helpers (no torch, no model): request checking, message conversion, tool-call
parsing, stop strings and SSE framing. Argv: <hf_worker.py>. Prints PASS.
Run by `python_hf::tests::chat_helpers_parse_and_frame` with any Python 3."""
import importlib.util
import json
import sys

spec = importlib.util.spec_from_file_location("hf_worker", sys.argv[1])
w = importlib.util.module_from_spec(spec)
spec.loader.exec_module(w)


def refused(fn, *a, **k):
    try:
        fn(*a, **k)
    except w.BadRequest as e:
        return str(e)
    raise AssertionError(f"{fn.__name__}{a} was accepted")


# --- tool calls ---------------------------------------------------------------------------------
names = {"get_weather", "add"}
qwen = (
    "Let me check.\n<tool_call>\n"
    '{"name": "get_weather", "arguments": {"city": "Paris"}}\n</tool_call>\n'
    '<tool_call>\n{"name": "add", "arguments": {"a": 1, "b": 2}}\n</tool_call>'
)
content, calls = w.parse_tool_calls(qwen, names)
assert content == "Let me check.", content
assert [c["function"]["name"] for c in calls] == ["get_weather", "add"]
assert json.loads(calls[0]["function"]["arguments"]) == {"city": "Paris"}
assert all(c["type"] == "function" and c["id"].startswith("call_") for c in calls)
assert len({c["id"] for c in calls}) == 2

# A call the model left open (it stopped at the token limit) still parses; so does `parameters`.
content, calls = w.parse_tool_calls('<tool_call>{"name": "add", "parameters": {"a": 1}}', names)
assert content == "" and calls[0]["function"]["arguments"] == '{"a": 1}', (content, calls)

# Other shapes: Mistral, Llama 3.1, a bare JSON answer, an OpenAI-shaped object.
_, calls = w.parse_tool_calls('[TOOL_CALLS][{"name": "add", "arguments": {"a": 3}}]', names)
assert calls and calls[0]["function"]["name"] == "add"
_, calls = w.parse_tool_calls('<|python_tag|>{"name": "get_weather", "parameters": {"city": "Rome"}}', names)
assert calls and json.loads(calls[0]["function"]["arguments"]) == {"city": "Rome"}
_, calls = w.parse_tool_calls('{"name": "add", "arguments": {"a": 5}}', names)
assert calls and calls[0]["function"]["name"] == "add"
_, calls = w.parse_tool_calls('{"function": {"name": "add", "arguments": "{\\"a\\": 7}"}}', names)
assert calls and calls[0]["function"]["arguments"] == '{"a": 7}'

# Not a call: a name that was not offered, prose that mentions JSON, plain text.
for text in (
    '<tool_call>{"name": "rm_rf", "arguments": {}}</tool_call>',
    'The answer is {"name": "add", "arguments": {}} as you can see.',
    "Paris is the capital of France.",
    "<tool_call>not json</tool_call>",
):
    content, calls = w.parse_tool_calls(text, names)
    assert calls == [] and content == text, (text, content, calls)

# --- stop strings -------------------------------------------------------------------------------
t = w.StopTrimmer(["END", "###"])
out = "".join(t.feed(piece) for piece in ["Hello wor", "ld EN", "D and more"])
assert out == "Hello world " and t.done and t.flush() == "", (out, t.done)
t = w.StopTrimmer(["STOP"])
out = "".join(t.feed(p) for p in ["no ", "stop", "ped here", " ST"]) + t.flush()
assert out == "no stopped here ST" and not t.done, out
t = w.StopTrimmer([])
assert t.feed("abc") == "abc" and t.flush() == ""

# --- request parameters -------------------------------------------------------------------------
p = w.chat_params({"max_tokens": 50, "temperature": 0.2, "stop": "x", "stream": True,
                   "stream_options": {"include_usage": True}, "seed": 7})
assert p["max_new_tokens"] == 50 and p["temperature"] == 0.2 and p["stop"] == ["x"]
assert p["stream"] and p["include_usage"] and p["seed"] == 7
assert w.chat_params({"max_completion_tokens": 9, "max_tokens": 5})["max_new_tokens"] == 9
assert w.chat_params({})["max_new_tokens"] is None and w.chat_params({})["stream"] is False
for bad in ({"n": 2}, {"stop": ["a"] * 5}, {"stop": [""]}, {"temperature": 9}, {"temperature": "hot"},
            {"max_tokens": 0}, {"top_p": True}, {"stop": 3}):
    refused(w.chat_params, bad)

# --- messages -----------------------------------------------------------------------------------
messages = [
    {"role": "developer", "content": "Be brief."},
    {"role": "user", "content": [{"type": "text", "text": "Hi "}, {"type": "text", "text": "there"}]},
    {"role": "assistant", "content": None, "tool_calls": [
        {"id": "call_1", "type": "function", "function": {"name": "add", "arguments": '{"a": 1}'}}]},
    {"role": "tool", "tool_call_id": "call_1", "content": "2"},
]
out, images = w.normalize_messages(messages, vision=False)
assert images == []
assert out[0] == {"role": "system", "content": "Be brief."}
assert out[1]["content"] == "Hi there"
assert out[2]["tool_calls"][0]["function"] == {"name": "add", "arguments": {"a": 1}}
assert out[3] == {"role": "tool", "content": "2", "tool_call_id": "call_1"}

image_message = [{"role": "user", "content": [
    {"type": "text", "text": "What is this?"},
    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}},
]}]
assert "does not take images" in refused(w.normalize_messages, image_message, False)
loaded = []
out, images = w.normalize_messages(image_message, True, load_image=lambda url: loaded.append(url) or "IMG")
assert images == ["IMG"] and loaded == ["data:image/png;base64,AAAA"]
assert out[0]["content"] == [{"type": "text", "text": "What is this?"}, {"type": "image"}]
out, _ = w.normalize_messages([{"role": "system", "content": "s"}], True)
assert out[0]["content"] == [{"type": "text", "text": "s"}]
refused(w.normalize_messages, [], False)
refused(w.normalize_messages, [{"role": "robot", "content": "x"}], False)
refused(w.normalize_messages, [{"role": "user", "content": [{"type": "audio"}]}], False)
refused(w.normalize_messages, [{"role": "assistant", "content": [
    {"type": "image_url", "image_url": {"url": "data:,"}}]}], True)
assert w.plain_prompt([{"role": "user", "content": "Hi"}]) == "user: Hi\nassistant:"

# --- images: only data: URLs and public http(s) addresses ---------------------------------------
for url in ("file:///etc/passwd", "ftp://example.com/a.png", "http://127.0.0.1:9/a.png",
            "http://localhost/a.png", "http://[::1]/a.png", "http://10.0.0.1/a.png",
            "http://169.254.169.254/latest/meta-data"):
    refused(w._fetch_image_bytes, url)

# --- SSE framing --------------------------------------------------------------------------------
assert w.sse(None) == b"data: [DONE]\n\n"
event = w.sse(w.chunk("id1", 5, "m", {"content": "hé"}))
assert event.startswith(b"data: {") and event.endswith(b"}\n\n")
body = json.loads(event[len(b"data: "):])
assert body == {"id": "id1", "object": "chat.completion.chunk", "created": 5, "model": "m",
                "choices": [{"index": 0, "delta": {"content": "hé"}, "finish_reason": None}]}
last = w.chunk("id1", 5, "m", {}, "stop")
assert last["choices"][0]["finish_reason"] == "stop"
usage = w.chunk("id1", 5, "m", None, usage=w.usage_of(3, 4))
assert usage["choices"] == [] and usage["usage"] == {"prompt_tokens": 3, "completion_tokens": 4, "total_tokens": 7}
done = w.completion("id2", 6, "m", {"role": "assistant", "content": "ok"}, "stop", w.usage_of(1, 2))
assert done["object"] == "chat.completion" and done["choices"][0]["message"]["content"] == "ok"
assert w.CHAT_TASKS <= w.TASKS

# --- audio in and speech out (Qwen2.5-Omni, any-to-any) -----------------------------------------
clips = []
fake_clip = lambda part: ("clip", part.get("format"))
msgs, imgs = w.normalize_messages(
    [{"role": "user", "content": [{"type": "text", "text": "what is said?"},
                                   {"type": "input_audio", "input_audio": {"data": "AAAA", "format": "wav"}}]}],
    True, audio=True, clips=clips, load_clip=fake_clip)
assert msgs == [{"role": "user", "content": [{"type": "text", "text": "what is said?"}, {"type": "audio"}]}]
assert clips == [("clip", "wav")] and imgs == []
# A text model refuses audio; audio is for user messages; the clip count is bounded.
assert "does not take audio" in refused(
    w.normalize_messages, [{"role": "user", "content": [{"type": "input_audio", "input_audio": {"data": "AA"}}]}],
    False, load_clip=fake_clip)
assert "only accepted in user" in refused(
    w.normalize_messages, [{"role": "assistant", "content": [{"type": "input_audio", "input_audio": {"data": "AA"}}]}],
    True, audio=True, clips=[], load_clip=fake_clip)
many = [{"role": "user", "content": [{"type": "input_audio", "input_audio": {"data": "AA"}}] * (w.MAX_CLIPS + 1)}]
assert "audio clips" in refused(w.normalize_messages, many, True, audio=True, clips=[], load_clip=fake_clip)


class Speaker:
    speaks = True


class Mute:
    speaks = False


assert w.speech_request({"messages": []}, Speaker()) is None
assert w.speech_request({"modalities": ["text"]}, Speaker()) is None
assert w.speech_request({"modalities": ["text", "audio"]}, Speaker()) == {"voice": "Chelsie"}
assert w.speech_request({"modalities": ["audio"], "audio": {"voice": "ETHAN", "format": "wav"}}, Speaker()) == {"voice": "Ethan"}
assert w.speech_request({"modalities": ["audio"], "audio": {"voice": "alloy"}}, Speaker()) == {"voice": "Chelsie"}
assert "does not answer with speech" in refused(w.speech_request, {"modalities": ["text", "audio"]}, Mute())
assert "modalities" in refused(w.speech_request, {"modalities": ["video"]}, Speaker())
assert "not served" in refused(w.speech_request, {"modalities": ["audio"], "audio": {"format": "mp3"}}, Speaker())
reply = w.audio_message(b"RIFFxxxxWAVE", "hello")
assert reply["transcript"] == "hello" and reply["id"].startswith("audio_")
import base64
assert base64.b64decode(reply["data"]) == b"RIFFxxxxWAVE"
assert w.CHAT_TASKS >= {"any-to-any"}

print("PASS")
