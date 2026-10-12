"""Tool-calling reliability suite: fixed cases, precise expectations, deterministic scoring."""
from __future__ import annotations

import json
import re
import unicodedata
from typing import Callable

# ---------------------------------------------------------------------------
# Tool definitions (OpenAI "tools" format)
# ---------------------------------------------------------------------------


def T(name: str, desc: str, props: dict, required: list[str]) -> dict:
    return {"type": "function", "function": {"name": name, "description": desc, "parameters": {
        "type": "object", "properties": props, "required": required}}}


S = {"type": "string"}
I = {"type": "integer"}
B = {"type": "boolean"}


def E(*vals: str) -> dict:
    return {"type": "string", "enum": list(vals)}


def A(item: dict) -> dict:
    return {"type": "array", "items": item}


get_weather = T("get_weather", "Get the current weather for a city.",
                {"city": {"type": "string", "description": "City name"}, "unit": E("celsius", "fahrenheit")}, ["city"])
calculate = T("calculate", "Evaluate an arithmetic expression and return the result.",
              {"expression": {"type": "string", "description": "e.g. '3 * (4 + 5)'"}}, ["expression"])
search_web = T("search_web", "Search the web and return result snippets.",
               {"query": S, "max_results": {"type": "integer", "description": "Number of results"}}, ["query"])
send_email = T("send_email", "Send an email message.", {"to": S, "subject": S, "body": S}, ["to", "subject", "body"])
create_event = T("create_event", "Create a calendar event.",
                 {"title": S, "date": {"type": "string", "description": "YYYY-MM-DD"}, "attendees": A(S)},
                 ["title", "date"])
read_file = T("read_file", "Read a text file from disk and return its contents.", {"path": S}, ["path"])
set_timer = T("set_timer", "Start a countdown timer.", {"minutes": I, "label": S}, ["minutes"])
translate = T("translate", "Translate text into another language.",
              {"text": S, "target_language": E("english", "french", "german", "spanish", "turkish")},
              ["text", "target_language"])

POOL8 = [get_weather, calculate, search_web, send_email, create_event, read_file, set_timer, translate]

create_order = T("create_order", "Create a customer order.", {
    "customer": {"type": "object", "properties": {"name": S, "address": {"type": "object", "properties": {"city": S, "zip": S}, "required": ["city", "zip"]}}, "required": ["name", "address"]},
    "items": A({"type": "object", "properties": {"sku": S, "qty": I}, "required": ["sku", "qty"]}),
    "priority": E("low", "normal", "urgent")}, ["customer", "items"])
update_config = T("update_config", "Update application settings.", {
    "settings": {"type": "object", "properties": {"theme": E("light", "dark"), "notifications": {
        "type": "object", "properties": {"email": B, "sms": B}}}}}, ["settings"])
book_flight = T("book_flight", "Book a flight.", {
    "passengers": A({"type": "object", "properties": {"name": S, "age": I}, "required": ["name", "age"]}),
    "route": {"type": "object", "properties": {"from": S, "to": S}, "required": ["from", "to"]},
    "cabin": E("economy", "business", "first")}, ["passengers", "route"])
set_log_level = T("set_log_level", "Set the logging level of the service.", {"level": E("debug", "info", "warn", "error")}, ["level"])
sort_list = T("sort_list", "Sort a list of numbers.", {"values": A({"type": "number"}), "order": E("asc", "desc")}, ["values", "order"])

EXTRA5 = [create_order, update_config, book_flight, set_log_level, sort_list]

_AGENT_RAW = [
    ("run_shell", "Run a shell command and return stdout/stderr.", {"command": S, "timeout_seconds": I}, ["command"]),
    ("write_file", "Write content to a file, replacing it.", {"path": S, "content": S}, ["path", "content"]),
    ("edit_file", "Replace exact text in a file.", {"path": S, "old_text": S, "new_text": S}, ["path", "old_text", "new_text"]),
    ("glob_files", "Find files matching a glob pattern.", {"pattern": S, "path": S}, ["pattern"]),
    ("grep_search", "Search file contents with a regular expression.", {"pattern": S, "path": S, "case_sensitive": B}, ["pattern"]),
    ("list_dir", "List a directory.", {"path": S}, ["path"]),
    ("web_fetch", "Fetch a URL and return its text.", {"url": S}, ["url"]),
    ("todo_write", "Replace the task list.", {"todos": A({"type": "object", "properties": {"content": S, "status": E("pending", "in_progress", "done")}})}, ["todos"]),
    ("spawn_subagent", "Delegate a task to a sub-agent.", {"description": S, "prompt": S}, ["description", "prompt"]),
    ("notebook_edit", "Edit a Jupyter notebook cell.", {"path": S, "cell": I, "source": S}, ["path", "cell", "source"]),
    ("git_status", "Show git status.", {"path": S}, []),
    ("git_diff", "Show git diff.", {"path": S, "staged": B}, []),
    ("git_commit", "Create a git commit.", {"message": S}, ["message"]),
    ("run_tests", "Run the project's tests.", {"filter": S}, []),
    ("lint", "Run the linter.", {"path": S, "fix": B}, []),
    ("format_code", "Format source files.", {"path": S}, ["path"]),
    ("install_package", "Install a package dependency.", {"name": S, "version": S}, ["name"]),
    ("docker_ps", "List running containers.", {"all": B}, []),
    ("http_request", "Send an HTTP request.", {"method": E("GET", "POST", "PUT", "DELETE"), "url": S, "body": S}, ["method", "url"]),
    ("db_query", "Run a read-only SQL query.", {"sql": S}, ["sql"]),
    ("take_screenshot", "Take a screenshot of the screen.", {"region": S}, []),
    ("open_url", "Open a URL in the browser.", {"url": S}, ["url"]),
    ("memory_store", "Store a note in long-term memory.", {"key": S, "value": S}, ["key", "value"]),
    ("memory_recall", "Recall a note from long-term memory.", {"key": S}, ["key"]),
    ("ask_user", "Ask the user a clarifying question.", {"question": S}, ["question"]),
]
AGENT25 = [T(*a) for a in _AGENT_RAW]
MANY38 = POOL8 + EXTRA5 + AGENT25  # 8 + 5 + 25 = 38
assert len(MANY38) == 38, len(MANY38)

# ---------------------------------------------------------------------------
# Matchers
# ---------------------------------------------------------------------------


def norm(s) -> str:
    s = str(s).strip().replace("İ", "i").replace("ı", "i").replace("I", "i")
    s = unicodedata.normalize("NFKD", s)
    return "".join(c for c in s if not unicodedata.combining(c)).lower()


def eq(v) -> Callable:
    return lambda x: norm(x) == norm(v)


def has(sub: str) -> Callable:
    return lambda x: norm(sub) in norm(x)


def num(n: float) -> Callable:
    def f(x):
        try:
            return float(x) == float(n)
        except (TypeError, ValueError):
            return False
    return f


def evalis(n: float) -> Callable:
    def f(x):
        s = str(x).replace("×", "*").replace("x", "*").replace("÷", "/").replace(",", ".")
        if not re.fullmatch(r"[0-9+\-*/(). ]+", s):
            return False
        try:
            return abs(float(eval(s, {"__builtins__": {}}, {})) - n) < 1e-6  # noqa: S307 - sanitized
        except Exception:  # noqa: BLE001
            return False
    return f


def lst(*items: str) -> Callable:
    def f(x):
        if not isinstance(x, list) or len(x) != len(items):
            return False
        got = [norm(i) for i in x]
        return all(any(norm(w) in g for g in got) for w in items)
    return f


def obj(**fields: Callable) -> Callable:
    def f(x):
        return isinstance(x, dict) and all(k in x and m(x[k]) for k, m in fields.items())
    return f


def objl(*matchers: Callable) -> Callable:
    """List of objects, each matching the corresponding matcher in order."""
    def f(x):
        return isinstance(x, list) and len(x) == len(matchers) and all(m(i) for m, i in zip(matchers, x))
    return f


def tf(v: bool) -> Callable:
    return lambda x: x is v or (isinstance(x, str) and x.lower() == str(v).lower())


def call(name: str, **args: Callable) -> tuple[str, dict]:
    return (name, args)


# ---------------------------------------------------------------------------
# Cases: (id, category, lang, tools, user_prompt, expected_calls)  expected [] = no call allowed
# ---------------------------------------------------------------------------

Case = tuple  # (id, category, lang, tools, prompt, expected)

CASES: list[Case] = [
    # single call, typed args
    ("single-weather", "single", "en", POOL8, "What's the weather in Paris right now? Use celsius.",
     [call("get_weather", city=eq("Paris"), unit=eq("celsius"))]),
    ("single-calc", "single", "en", POOL8, "Compute (17 * 23) + 4 with the calculator.",
     [call("calculate", expression=evalis(395))]),
    ("single-timer", "single", "en", POOL8, "Set a timer for 25 minutes.",
     [call("set_timer", minutes=num(25))]),
    ("single-readfile", "single", "en", POOL8, "Please read the file /etc/hosts.",
     [call("read_file", path=eq("/etc/hosts"))]),
    ("single-search", "single", "en", POOL8, "Search the web for 'rust async runtime comparison' and give me 5 results.",
     [call("search_web", query=has("rust async runtime"), max_results=num(5))]),
    # choose the right tool among 8
    ("choose-email", "choose", "en", POOL8, "Email bob@example.com with subject 'Lunch' and the body 'See you at noon'.",
     [call("send_email", to=eq("bob@example.com"), subject=eq("Lunch"), body=has("noon"))]),
    ("choose-translate", "choose", "en", POOL8, "Translate 'Good morning' into French.",
     [call("translate", text=has("good morning"), target_language=eq("french"))]),
    ("choose-event", "choose", "en", POOL8, "Create a calendar event 'Team sync' on 2026-03-14 with alice and bob.",
     [call("create_event", title=eq("Team sync"), date=eq("2026-03-14"), attendees=lst("alice", "bob"))]),
    ("choose-weather-f", "choose", "en", POOL8, "How hot is it in Tokyo? Give it in fahrenheit.",
     [call("get_weather", city=eq("Tokyo"), unit=eq("fahrenheit"))]),
    ("choose-timer", "choose", "en", POOL8, "Remind me in 90 minutes, label it 'tea'.",
     [call("set_timer", minutes=num(90), label=has("tea"))]),
    # parallel (2 calls)
    ("parallel-weather2", "parallel", "en", POOL8, "Get the weather for Berlin and for Madrid.",
     [call("get_weather", city=eq("Berlin")), call("get_weather", city=eq("Madrid"))]),
    ("parallel-mixed", "parallel", "en", POOL8, "Tell me the weather in London and also compute 12 * 12.",
     [call("get_weather", city=eq("London")), call("calculate", expression=evalis(144))]),
    ("parallel-read2", "parallel", "en", POOL8, "Read both a.txt and b.txt.",
     [call("read_file", path=eq("a.txt")), call("read_file", path=eq("b.txt"))]),
    # no tool needed
    ("none-capital", "none", "en", POOL8, "What is the capital of France?", []),
    ("none-hello", "none", "en", POOL8, "Say hello in one sentence.", []),
    ("none-explain", "none", "en", POOL8, "Explain what a hash map is in two sentences.", []),
    ("none-haiku", "none", "en", POOL8, "Write a haiku about the sea.", []),
    # nested JSON args
    ("nested-order", "nested", "en", [create_order, send_email, read_file],
     "Create an urgent order for customer Maria Lopez living in Madrid, zip 28001, with 2 of SKU A-100 and 1 of SKU B-200.",
     [call("create_order", customer=obj(name=has("maria lopez"), address=obj(city=eq("Madrid"), zip=eq("28001"))),
           items=objl(obj(sku=eq("A-100"), qty=num(2)), obj(sku=eq("B-200"), qty=num(1))), priority=eq("urgent"))]),
    ("nested-config", "nested", "en", [update_config, set_log_level, read_file],
     "Switch the app to the dark theme, enable email notifications and disable sms notifications.",
     [call("update_config", settings=obj(theme=eq("dark"), notifications=obj(email=tf(True), sms=tf(False))))]),
    ("nested-flight", "nested", "en", [book_flight, create_event, search_web],
     "Book a business class flight from Istanbul to Berlin for John (34) and Anna (31).",
     [call("book_flight", passengers=objl(obj(name=has("john"), age=num(34)), obj(name=has("anna"), age=num(31))),
           route=obj(**{"from": has("istanbul"), "to": has("berlin")}), cabin=eq("business"))]),
    # enum args
    ("enum-translate", "enum", "en", POOL8, "Translate 'Where is the station?' to German.",
     [call("translate", text=has("where is the station"), target_language=eq("german"))]),
    ("enum-loglevel", "enum", "en", [set_log_level, read_file, calculate], "Turn the logging level down to warn.",
     [call("set_log_level", level=eq("warn"))]),
    ("enum-sort", "enum", "en", [sort_list, calculate, read_file], "Sort 5, 3, 9, 1 in descending order.",
     [call("sort_list", values=lambda x: isinstance(x, list) and [float(i) for i in x] == [5, 3, 9, 1], order=eq("desc"))]),
    # Turkish
    ("tr-weather", "turkish", "tr", POOL8, "İstanbul'da hava nasıl? Celsius olarak ver.",
     [call("get_weather", city=eq("İstanbul"), unit=eq("celsius"))]),
    ("tr-timer", "turkish", "tr", POOL8, "25 dakikalık bir zamanlayıcı kur.",
     [call("set_timer", minutes=num(25))]),
    ("tr-parallel", "turkish", "tr", POOL8, "Ankara ve İzmir için hava durumunu getir.",
     [call("get_weather", city=eq("Ankara")), call("get_weather", city=eq("İzmir"))]),
    ("tr-translate", "turkish", "tr", POOL8, "'Günaydın' kelimesini İngilizceye çevir.",
     [call("translate", text=has("günaydın"), target_language=eq("english"))]),
    ("tr-email", "turkish", "tr", POOL8, "ali@example.com adresine 'Toplantı' konulu, içeriği 'Yarın 10'da görüşelim' olan bir e-posta gönder.",
     [call("send_email", to=eq("ali@example.com"), subject=eq("Toplantı"), body=has("yarın 10"))]),
    ("tr-readfile", "turkish", "tr", POOL8, "/home/user/notlar.txt dosyasını oku.",
     [call("read_file", path=eq("/home/user/notlar.txt"))]),
    ("tr-event", "turkish", "tr", POOL8, "Takvime 2026-05-20 tarihine 'Doğum günü' etkinliği ekle, katılımcı Ayşe.",
     [call("create_event", title=has("doğum günü"), date=eq("2026-05-20"), attendees=lst("ayşe"))]),
    ("tr-calc", "turkish", "tr", POOL8, "345 çarpı 12 kaç eder? Hesap makinesi aracını kullan.",
     [call("calculate", expression=evalis(4140))]),
    ("tr-none", "turkish", "tr", POOL8, "Fransa'nın başkenti neresi?", []),
    ("tr-none-poem", "turkish", "tr", POOL8, "Bana denizle ilgili kısa bir şiir yaz.", []),
    # many tools (38 definitions, agent-client style)
    ("many-read", "many_tools", "en", MANY38, "Read the file src/main.rs", [call("read_file", path=eq("src/main.rs"))]),
    ("many-weather", "many_tools", "en", MANY38, "What's the weather in Oslo? Celsius please.",
     [call("get_weather", city=eq("Oslo"), unit=eq("celsius"))]),
    ("many-grep", "many_tools", "en", MANY38, "Search the src directory for the text TODO using grep.",
     [call("grep_search", pattern=has("TODO"), path=has("src"))]),
    ("many-glob-tr", "many_tools", "tr", MANY38, "src klasöründeki tüm *.rs dosyalarını bul.",
     [call("glob_files", pattern=has("*.rs"))]),
    ("many-none", "many_tools", "en", MANY38, "What does HTTP stand for?", []),
]

SYSTEM = "You are a helpful assistant. Use the provided tools when they are needed; otherwise answer directly."

_TEXT_CALL = re.compile(r"<tool_call>|\"name\"\s*:\s*\"[A-Za-z_]+\"[^}]*\"arguments\"|\[TOOL_CALLS\]|<function=|<\|python_tag\|>")


def score_case(case: Case, resp: dict) -> dict:
    cid, cat, lang, _tools, _prompt, expected = case
    out = {"id": cid, "category": cat, "lang": lang, "pass": False, "reason": "", "text_format_call": False,
           "n_calls": 0, "latency_s": resp.get("wall")}
    if not resp.get("ok"):
        out["reason"] = "request_error: " + str(resp.get("error"))[:200]
        return out
    msg = resp["message"]
    content = msg.get("content") or ""
    calls = msg.get("tool_calls") or []
    out["n_calls"] = len(calls)
    out["text_format_call"] = bool(_TEXT_CALL.search(content))
    out["content_head"] = content[:160]
    out["calls"] = [{"name": c["function"]["name"], "arguments": c["function"]["arguments"]} for c in calls]
    if not expected:
        if calls:
            out["reason"] = "spurious_call:" + ",".join(c["function"]["name"] for c in calls)
        elif out["text_format_call"]:
            out["reason"] = "spurious_text_call"
        elif not content.strip():
            out["reason"] = "empty_answer"
        else:
            out["pass"] = True
        return out
    if not calls:
        out["reason"] = "text_format_call_not_converted" if out["text_format_call"] else "no_tool_call"
        return out
    if len(calls) != len(expected):
        out["reason"] = f"wrong_call_count:{len(calls)}!={len(expected)}"
        return out
    parsed = []
    for c in calls:
        try:
            a = c["function"]["arguments"]
            parsed.append((c["function"]["name"], json.loads(a) if isinstance(a, str) else a))
        except Exception:  # noqa: BLE001
            out["reason"] = "arguments_not_json"
            return out
    remaining = list(parsed)
    for en, em in expected:
        hit = None
        for i, (n, a) in enumerate(remaining):
            if n == en and isinstance(a, dict) and all(k in a and m(a[k]) for k, m in em.items()):
                hit = i
                break
        if hit is None:
            same = [a for n, a in remaining if n == en]
            out["reason"] = f"{'arg_mismatch' if same else 'wrong_tool'}:{en}"
            return out
        remaining.pop(hit)
    out["pass"] = True
    return out
