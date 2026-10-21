//! A small tool-calling probe: a handful of fixed requests with tool definitions against a
//! running OpenAI-compatible server, each checked for a well-formed call (right tool, valid
//! JSON arguments with the expected values) or, where no tool fits, for no call at all.
//!
//! The pass rate feeds the quality score of the recommendations ([`crate::model_perf`]). The
//! suite is deliberately small (seven requests, temperature 0) so it finishes in a minute on a
//! slow machine; it measures whether a model can call tools at all, not how well it plans.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// Per-request limit; a slow CPU model still answers a short tool request well inside it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// What a case expects of the response.
enum Expect {
    /// One call to `name` whose arguments pass these checks.
    Call(&'static str, &'static [Arg]),
    /// A plain answer: no tool call.
    NoCall,
}

/// A check on one argument of a call.
enum Arg {
    /// A string containing this text (case-insensitive).
    Contains(&'static str, &'static str),
    /// A number equal to this value.
    Number(&'static str, f64),
    /// A string equal to one of these (case-insensitive).
    OneOf(&'static str, &'static [&'static str]),
    /// Present and not empty.
    Present(&'static str),
}

struct Case {
    id: &'static str,
    prompt: &'static str,
    expect: Expect,
}

const CASES: &[Case] = &[
    Case {
        id: "string_argument",
        prompt: "What is the weather in Paris right now?",
        expect: Expect::Call("get_weather", &[Arg::Contains("city", "paris")]),
    },
    Case {
        id: "numeric_arguments",
        prompt: "Add 17 and 25 using the add tool.",
        expect: Expect::Call("add", &[Arg::Number("a", 17.0), Arg::Number("b", 25.0)]),
    },
    Case {
        id: "enum_argument",
        prompt: "Turn the kitchen light off.",
        expect: Expect::Call(
            "set_light",
            &[
                Arg::Contains("room", "kitchen"),
                Arg::OneOf("state", &["off"]),
            ],
        ),
    },
    Case {
        id: "tool_choice",
        prompt: "Search the web for rust async runtimes.",
        expect: Expect::Call("search_web", &[Arg::Contains("query", "rust")]),
    },
    Case {
        id: "required_fields",
        prompt: "Create a calendar event titled Team Sync on 2030-05-04.",
        expect: Expect::Call(
            "create_event",
            &[Arg::Contains("title", "team sync"), Arg::Present("date")],
        ),
    },
    Case {
        id: "second_tool",
        prompt: "Convert 100 kilometers to miles with the unit converter.",
        expect: Expect::Call(
            "convert_units",
            &[
                Arg::Number("value", 100.0),
                Arg::Contains("from", "k"),
                Arg::Contains("to", "mile"),
            ],
        ),
    },
    Case {
        id: "no_tool_needed",
        prompt: "Say hello in one short sentence. Do not use any tool.",
        expect: Expect::NoCall,
    },
];

fn tools() -> Value {
    let f = |name: &str, desc: &str, props: Value, required: Value| {
        json!({"type": "function", "function": {
            "name": name, "description": desc,
            "parameters": {"type": "object", "properties": props, "required": required}
        }})
    };
    json!([
        f(
            "get_weather",
            "Current weather of a city.",
            json!({"city": {"type": "string", "description": "City name"}}),
            json!(["city"])
        ),
        f(
            "add",
            "Add two numbers.",
            json!({"a": {"type": "number"}, "b": {"type": "number"}}),
            json!(["a", "b"])
        ),
        f(
            "set_light",
            "Switch a light on or off.",
            json!({"room": {"type": "string"}, "state": {"type": "string", "enum": ["on", "off"]}}),
            json!(["room", "state"])
        ),
        f(
            "search_web",
            "Search the web.",
            json!({"query": {"type": "string"}}),
            json!(["query"])
        ),
        f(
            "create_event",
            "Create a calendar event.",
            json!({"title": {"type": "string"}, "date": {"type": "string", "description": "YYYY-MM-DD"}}),
            json!(["title", "date"])
        ),
        f(
            "convert_units",
            "Convert a value between units.",
            json!({"value": {"type": "number"}, "from": {"type": "string"}, "to": {"type": "string"}}),
            json!(["value", "from", "to"])
        ),
    ])
}

/// Result of one probe run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolProbeResult {
    pub passed: u32,
    pub total: u32,
    /// Ids of the cases that failed.
    #[serde(default)]
    pub failed: Vec<String>,
    /// Unix seconds.
    pub at: u64,
}

impl ToolProbeResult {
    /// Share of passed cases, 0 to 1.
    pub fn rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.passed as f64 / self.total as f64
        }
    }
}

fn arg_ok(args: &Value, check: &Arg) -> bool {
    match check {
        Arg::Contains(key, text) => args
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| s.to_lowercase().contains(text)),
        Arg::Number(key, want) => args.get(key).is_some_and(|v| {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                .is_some_and(|n| (n - want).abs() < 1e-9)
        }),
        Arg::OneOf(key, options) => args
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|s| options.contains(&s.to_lowercase().as_str())),
        Arg::Present(key) => args.get(key).is_some_and(|v| match v {
            Value::Null => false,
            Value::String(s) => !s.trim().is_empty(),
            _ => true,
        }),
    }
}

/// Whether a chat completion response meets the case.
fn passes(expect: &Expect, response: &Value) -> bool {
    let message = &response["choices"][0]["message"];
    let calls = message["tool_calls"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    match expect {
        Expect::NoCall => calls.is_empty(),
        Expect::Call(name, checks) => {
            let [call] = calls.as_slice() else {
                return false;
            };
            if call["function"]["name"].as_str() != Some(name) {
                return false;
            }
            // Arguments are a JSON string in the OpenAI shape; some servers send an object.
            let raw = &call["function"]["arguments"];
            let args = match raw {
                Value::String(s) => serde_json::from_str::<Value>(s).ok(),
                Value::Object(_) => Some(raw.clone()),
                _ => None,
            };
            args.is_some_and(|a| a.is_object() && checks.iter().all(|c| arg_ok(&a, c)))
        }
    }
}

/// Runs the suite against `base_url` (`http://127.0.0.1:8080`), asking for `model`. A request
/// that errors or times out fails its case; the run itself only fails when no request could
/// reach the server at all.
pub async fn run(
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
) -> Result<ToolProbeResult, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let tools = tools();
    let mut result = ToolProbeResult::default();
    let mut reached = false;
    for case in CASES {
        let body = json!({
            "model": model,
            "messages": [{"role": "user", "content": case.prompt}],
            "tools": tools,
            "tool_choice": "auto",
            "temperature": 0,
            "max_tokens": 512,
            "stream": false,
        });
        let mut req = client.post(&url).json(&body);
        if let Some(key) = api_key {
            req = req.bearer_auth(key);
        }
        let ok = match req.send().await {
            Ok(res) => {
                reached = true;
                match res.json::<Value>().await {
                    Ok(v) => passes(&case.expect, &v),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };
        result.total += 1;
        if ok {
            result.passed += 1;
        } else {
            result.failed.push(case.id.to_string());
        }
    }
    if !reached {
        return Err(crate::error_key::err("tool_probe.unreachable").into());
    }
    result.at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: Value) -> Value {
        json!({"choices": [{"message": {"tool_calls": [
            {"function": {"name": name, "arguments": args.to_string()}}
        ]}}]})
    }

    fn case(id: &str) -> &'static Case {
        CASES.iter().find(|c| c.id == id).unwrap()
    }

    #[test]
    fn accepts_well_formed_calls() {
        let ok = |id: &str, r: Value| passes(&case(id).expect, &r);
        assert!(ok(
            "string_argument",
            call("get_weather", json!({"city": "Paris, France"}))
        ));
        assert!(ok(
            "numeric_arguments",
            call("add", json!({"a": 17, "b": "25"}))
        ));
        assert!(ok(
            "enum_argument",
            call("set_light", json!({"room": "Kitchen", "state": "OFF"}))
        ));
        // An object instead of a JSON string.
        let object_args = json!({"choices": [{"message": {"tool_calls": [
            {"function": {"name": "search_web", "arguments": {"query": "rust async"}}}
        ]}}]});
        assert!(ok("tool_choice", object_args));
    }

    #[test]
    fn rejects_wrong_tool_bad_json_missing_or_extra_calls() {
        let bad = |id: &str, r: Value| !passes(&case(id).expect, &r);
        assert!(bad(
            "string_argument",
            call("add", json!({"city": "Paris"}))
        ));
        assert!(bad(
            "numeric_arguments",
            call("add", json!({"a": 17, "b": 24}))
        ));
        assert!(bad(
            "required_fields",
            call("create_event", json!({"title": "Team Sync"}))
        ));
        assert!(bad(
            "required_fields",
            call("create_event", json!({"title": "Team Sync", "date": " "}))
        ));
        let broken = json!({"choices": [{"message": {"tool_calls": [
            {"function": {"name": "get_weather", "arguments": "{city: Paris"}}
        ]}}]});
        assert!(bad("string_argument", broken));
        let text_only = json!({"choices": [{"message": {"content": "It is sunny."}}]});
        assert!(bad("string_argument", text_only.clone()));
        let two = json!({"choices": [{"message": {"tool_calls": [
            {"function": {"name": "add", "arguments": "{\"a\":17,\"b\":25}"}},
            {"function": {"name": "add", "arguments": "{\"a\":17,\"b\":25}"}}
        ]}}]});
        assert!(bad("numeric_arguments", two));
        // No-call case: text passes, a call fails.
        assert!(passes(&case("no_tool_needed").expect, &text_only));
        assert!(!passes(
            &case("no_tool_needed").expect,
            &call("search_web", json!({"query": "hello"}))
        ));
    }

    #[test]
    fn result_rate() {
        let r = ToolProbeResult {
            passed: 5,
            total: 7,
            ..Default::default()
        };
        assert!((r.rate() - 5.0 / 7.0).abs() < 1e-9);
        assert_eq!(ToolProbeResult::default().rate(), 0.0);
    }

    #[tokio::test]
    async fn an_unreachable_server_is_an_error_not_a_zero_score() {
        let e = run("http://127.0.0.1:9", "m", None).await.unwrap_err();
        assert!(e.contains("tool_probe.unreachable"), "{e}");
    }
}
