//! Bound one Chat Completions turn before tool calls reach external clients.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

// Shared with Playground; the retention rationale and limits live in shared/tool-loop-policy.json.
static POLICY: std::sync::LazyLock<Value> = std::sync::LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../shared/tool-loop-policy.json")).unwrap()
});
fn setting(name: &str) -> usize {
    POLICY[name].as_u64().unwrap() as usize
}
fn limit(offered: usize) -> usize {
    let max = setting("max_calls");
    if offered >= setting("menu_min_tools") {
        max.min(
            (offered * setting("menu_ratio_numerator"))
                .div_ceil(setting("menu_ratio_denominator"))
                .max(setting("menu_min_calls")),
        )
    } else {
        max
    }
}
fn retained(repeat_index: usize) -> usize {
    if repeat_index <= setting("short_repeat_prefix") {
        repeat_index
    } else {
        1
    }
}
fn canonical(v: Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| (k, canonical(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(canonical).collect()),
        v => v,
    }
}
fn key(c: &Value) -> String {
    let args = c["function"]["arguments"].as_str().unwrap_or("");
    let normalized = serde_json::from_str(args)
        .map(canonical)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| args.trim().into());
    json!([c["function"]["name"].as_str().unwrap_or(""), normalized]).to_string()
}
#[derive(Default)]
struct Turn {
    calls: BTreeMap<usize, Value>,
    sent: HashSet<usize>,
}
pub struct Guard {
    wire: Vec<u8>,
    turns: BTreeMap<usize, Turn>,
    limit: usize,
    pub stopped: bool,
    pub count: usize,
}
impl Guard {
    pub fn new(offered: usize) -> Self {
        Self {
            wire: vec![],
            turns: BTreeMap::new(),
            limit: limit(offered),
            stopped: false,
            count: 0,
        }
    }
    pub fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        if self.stopped {
            return vec![];
        }
        self.wire.extend_from_slice(bytes);
        let mut out = vec![];
        while let Some(end) = self
            .wire
            .windows(2)
            .position(|w| w == b"\n\n")
            .map(|i| i + 2)
            .or_else(|| {
                self.wire
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|i| i + 4)
            })
        {
            let frame: Vec<_> = self.wire.drain(..end).collect();
            let data = String::from_utf8_lossy(&frame)
                .lines()
                .filter_map(|l| l.strip_prefix("data:").map(str::trim))
                .collect::<Vec<_>>()
                .join("\n");
            let Ok(mut v) = serde_json::from_str::<Value>(&data) else {
                out.extend(frame);
                continue;
            };
            if !v["choices"].is_array() {
                out.extend(frame);
                continue;
            }
            if let Some(choices) = v["choices"].as_array_mut() {
                let mut keep = choices.len();
                for (position, choice) in choices.iter_mut().enumerate() {
                    let ci = choice["index"].as_u64().unwrap_or(0) as usize;
                    let turn = self.turns.entry(ci).or_default();
                    if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                        for c in calls {
                            let index = c["index"].as_u64().unwrap_or(0) as usize;
                            let entry = turn.calls.entry(index).or_insert_with(|| json!({"index":index,"type":"function","function":{"name":"","arguments":""}}));
                            if let Some(id) = c.get("id") {
                                entry["id"] = id.clone();
                            }
                            for field in ["name", "arguments"] {
                                if let Some(s) = c["function"][field].as_str() {
                                    let previous = entry["function"][field].as_str().unwrap_or("");
                                    entry["function"][field] =
                                        if field == "name" && s.starts_with(previous) {
                                            json!(s)
                                        } else {
                                            json!(format!("{previous}{s}"))
                                        };
                                }
                            }
                        }
                        choice["delta"]
                            .as_object_mut()
                            .unwrap()
                            .remove("tool_calls");
                    }
                    let finishing = choice["finish_reason"].is_string();
                    let mut keys = HashSet::new();
                    let mut decision = None;
                    for (i, call) in turn.calls.values().enumerate() {
                        if i >= self.limit {
                            decision = Some((1, i + 1));
                            break;
                        }
                        let args = call["function"]["arguments"].as_str().unwrap_or("");
                        if !finishing
                            && (call["function"]["name"].as_str().unwrap_or("").is_empty()
                                || serde_json::from_str::<Value>(args).is_err())
                        {
                            // A partial earlier call must not hide count overflow.
                            continue;
                        }
                        if !keys.insert(key(call)) {
                            decision = Some((retained(i), i + 1));
                            break;
                        }
                    }
                    if let Some((_, count)) = decision {
                        self.stopped = true;
                        self.count = count;
                    }
                    let mut ready = vec![];
                    if finishing || decision.is_some() {
                        let keep = decision.map_or(turn.calls.len(), |(keep, _)| keep);
                        for (index, call) in turn.calls.iter().take(keep) {
                            if turn.sent.insert(*index) {
                                ready.push(call.clone());
                            }
                        }
                    }
                    if !ready.is_empty() {
                        choice["delta"]["tool_calls"] = json!(ready);
                    }
                    if self.stopped {
                        choice["finish_reason"] = json!("tool_calls");
                        keep = position + 1;
                        break;
                    }
                }
                choices.truncate(keep);
                if self.stopped {
                    for choice in choices {
                        let ci = choice["index"].as_u64().unwrap_or(0) as usize;
                        choice["finish_reason"] =
                            json!(if self.turns.get(&ci).is_some_and(|t| !t.sent.is_empty()) {
                                "tool_calls"
                            } else {
                                "stop"
                            });
                    }
                }
            }
            out.extend(format!("data: {v}\n\n").bytes());
            if self.stopped {
                out.extend(b"data: [DONE]\n\n");
                self.wire.clear();
                break;
            }
        }
        out
    }
    pub fn accepted_ids(&self) -> HashSet<String> {
        self.turns
            .values()
            .flat_map(|t| {
                t.sent
                    .iter()
                    .filter_map(|i| t.calls[i]["id"].as_str().map(str::to_string))
            })
            .collect()
    }
    pub fn finish(&mut self) -> Vec<u8> {
        if self.wire.is_empty() || self.stopped {
            return vec![];
        }
        self.push(b"\n\n")
    }
}
pub fn trim_json(bytes: &[u8], offered: usize) -> (Vec<u8>, Option<usize>) {
    let Ok(mut v) = serde_json::from_slice::<Value>(bytes) else {
        return (bytes.to_vec(), None);
    };
    let mut cut = None;
    if let Some(choices) = v["choices"].as_array_mut() {
        for choice in choices {
            if let Some(calls) = choice["message"]["tool_calls"].as_array_mut() {
                let mut keys = HashSet::new();
                let keep = calls.iter().enumerate().find_map(|(i, c)| {
                    if i >= limit(offered) {
                        Some((1, i + 1))
                    } else if !keys.insert(key(c)) {
                        Some((retained(i), i + 1))
                    } else {
                        None
                    }
                });
                if let Some((keep, count)) = keep {
                    calls.truncate(keep);
                    cut = Some(count);
                    choice["finish_reason"] = json!("tool_calls");
                }
            }
        }
    }
    (serde_json::to_vec(&v).unwrap(), cut)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn emitted(wire: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(wire)
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .flat_map(|v| {
                v["choices"][0]["delta"]["tool_calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }
    #[test]
    fn retention_matches_for_stream_and_json() {
        for (names, keep, stopped) in [
            (vec!["A", "B", "A", "B"], 2, true),
            (vec!["A", "B", "C", "A"], 3, true),
            (vec!["A", "B", "C", "D", "A"], 1, true),
            (vec!["A", "B", "C"], 3, false),
            (vec!["A", "B", "C", "D", "E", "F", "G", "H", "I"], 1, true),
        ] {
            let calls: Vec<_> = names.iter().enumerate().map(|(i, name)|
                json!({"index":i,"id":format!("original{i}"),"type":"function","function":{"name":name,"arguments":"{}"}})).collect();
            let mut g = Guard::new(16);
            let mut out = vec![];
            for c in &calls {
                let frame = format!(
                    "data: {}\n\n",
                    json!({"choices":[{"index":0,"delta":{"content":"text","tool_calls":[c]},"finish_reason":null}]})
                );
                let bytes = g.push(frame.as_bytes());
                if !g.stopped {
                    assert!(
                        emitted(&bytes).is_empty(),
                        "tool-call bytes escaped before finish/decision"
                    );
                    assert!(String::from_utf8_lossy(&bytes).contains("text"));
                }
                out.extend(bytes);
            }
            out.extend(g.push(b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n"));
            assert_eq!(g.stopped, stopped);
            assert_eq!(emitted(&out), calls[..keep]);
            assert_eq!(g.accepted_ids().len(), keep);
            let v = json!({"choices":[{"message":{"tool_calls":calls}}]});
            let (out, cut) = trim_json(&serde_json::to_vec(&v).unwrap(), 16);
            assert_eq!(cut.is_some(), stopped);
            assert_eq!(
                serde_json::from_slice::<Value>(&out).unwrap()["choices"][0]["message"]
                    ["tool_calls"],
                json!(calls[..keep])
            );
        }
    }

    #[test]
    fn trailing_event_without_separator_is_also_guarded() {
        let event = |i| {
            format!(
                "data: {}",
                json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":i,"function":{"name":"read","arguments":"{}"}}]},"finish_reason":null}]})
            )
        };
        let mut g = Guard::new(16);
        let first = format!("{}\n\n", event(0));
        assert!(!String::from_utf8(g.push(first.as_bytes()))
            .unwrap()
            .contains("\"tool_calls\":["));
        assert!(g.push(event(1).as_bytes()).is_empty());
        let out = String::from_utf8(g.finish()).unwrap();
        assert!(g.stopped);
        assert!(out.ends_with("data: [DONE]\n\n"));
        assert!(out.contains("\"tool_calls\":["));
    }

    #[test]
    fn stream_repeat_is_withheld_and_parallel_fragments_survive() {
        let event = |index: usize, name: &str, args: &str| {
            format!(
                "data: {}\r\n\r\n",
                json!({"id":"response","model":"test","choices":[{"index":0,"delta":{"tool_calls":[{"index":index,"id":format!("c{index}"),"type":"function","function":{"name":name,"arguments":args}}]},"finish_reason":null}]})
            )
        };
        let mut g = Guard::new(16);
        let mut out = g.push(event(0, "read", r#"{"nested":{"b":2,"a":1}}"#).as_bytes());
        assert!(
            !String::from_utf8_lossy(&out).contains("\"tool_calls\":["),
            "tool calls leaked before finish/decision"
        );
        for i in 1..4 {
            out.extend(g.push(event(i, "read", r#"{"n":"#).as_bytes()));
            out.extend(g.push(event(i, "", &format!("{i}}}")).as_bytes()));
        }
        assert!(!g.stopped);
        out.extend(g.push(event(4, "read", r#"{ "nested": { "a":1,"b":2 } }"#).as_bytes()));
        assert!(g.stopped);
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("\"id\":\"c4\""));
        assert_eq!(out.matches("\"type\":\"function\"").count(), 1);
        assert!(out.ends_with("data: [DONE]\n\n"));
        assert!(g.push(event(5, "read", "{}").as_bytes()).is_empty());
    }
    #[test]
    fn menu_enumeration_and_incomplete_calls_are_bounded() {
        let calls: Vec<_> = (0..16)
            .map(|i| json!({"index":i,"function":{"name":format!("tool{i}"),"arguments":"{"}}))
            .collect();
        let wire = format!(
            "data: {}\n\n",
            json!({"choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":null}]})
        );
        let mut g = Guard::new(8);
        let out = String::from_utf8(g.push(wire.as_bytes())).unwrap();
        assert!(g.stopped);
        assert_eq!(g.count, 7);
        assert!(out.ends_with("data: [DONE]\n\n"));
        assert!(!out.contains("tool6"));
        let value = json!({"choices":[{"message":{"tool_calls":(0..16).map(|i|json!({"function":{"name":format!("tool{i}"),"arguments":"{}"}})).collect::<Vec<_>>()}}]});
        let (out, cut) = trim_json(&serde_json::to_vec(&value).unwrap(), 16);
        assert_eq!(cut, Some(9));
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["choices"][0]["message"]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn real_runaway_keeps_only_search_even_with_byte_chunks() {
        let fixture = include_bytes!("../tests/fixtures/runaway-stream.txt");
        let mut g = Guard::new(16);
        let mut out = vec![];
        for b in fixture {
            out.extend(g.push(&[*b]));
        }
        let out = String::from_utf8(out).unwrap();
        assert!(g.stopped);
        assert!(out.ends_with("data: [DONE]\n\n"));
        let calls: Vec<Value> = out
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .filter_map(|s| serde_json::from_str::<Value>(s).ok())
            .flat_map(|v| {
                v["choices"][0]["delta"]["tool_calls"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["function"]["name"], "web_search");
        assert_eq!(
            serde_json::from_str::<Value>(calls[0]["function"]["arguments"].as_str().unwrap())
                .unwrap()["query"],
            "llama.cpp son s\u{00fc}r\u{00fc}m\u{00fc}"
        );
        assert!(out.contains("\"finish_reason\":\"tool_calls\""));
    }
    #[test]
    fn parallel_and_normalized_repeat() {
        let call = |args: &str| json!({"function":{"name":"read","arguments":args}});
        let v = json!({"choices":[{"message":{"tool_calls":[call("{\"b\":2,\"a\":1}"),call("{\"a\":2}"),call("{ \"a\":1, \"b\":2 }")]}}]});
        let (out, cut) = trim_json(&serde_json::to_vec(&v).unwrap(), 16);
        assert_eq!(cut, Some(3));
        assert_eq!(
            serde_json::from_slice::<Value>(&out).unwrap()["choices"][0]["message"]["tool_calls"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        let v = json!({"choices":[{"message":{"tool_calls":(0..4).map(|i|call(&format!("{{\"n\":{i}}}"))).collect::<Vec<_>>()}}]});
        assert_eq!(trim_json(&serde_json::to_vec(&v).unwrap(), 8).1, None);
    }
    #[test]
    fn three_parallel_calls_are_withheld_until_finish_while_content_flows() {
        let mut g = Guard::new(16);
        let calls: Vec<_> = (0..3).map(|i| json!({"index":i,"id":format!("original{i}"),"type":"function","function":{"name":format!("tool{i}"),"arguments":"{}"}})).collect();
        for call in &calls {
            let wire = format!(
                "data: {}\n\n",
                json!({"choices":[{"index":0,"delta":{"content":"hello","tool_calls":[call]},"finish_reason":null}]})
            );
            let out = g.push(wire.as_bytes());
            assert!(emitted(&out).is_empty());
            assert!(!String::from_utf8_lossy(&out).contains("tool_calls"));
            assert!(String::from_utf8_lossy(&out).contains("hello"));
            assert!(g.accepted_ids().is_empty());
        }
        let out = g.push(b"data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n");
        assert_eq!(emitted(&out), calls);
        assert!(!g.stopped);
        let value = json!({"choices":[{"message":{"tool_calls":calls}}]});
        assert_eq!(trim_json(&serde_json::to_vec(&value).unwrap(), 16).1, None);
    }
}
