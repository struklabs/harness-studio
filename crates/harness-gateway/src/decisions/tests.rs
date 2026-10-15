//! Compatibility of the decision APIs with their documentation: Ollama's `/v1/systemone`
//! reference, LM Studio's System One and Decisions pages, llama.cpp's server README (System
//! One section) and OpenAI's Decisions guide. Upstream answers are real llama-server b11574
//! responses (vllm-d3-edge Q8_0, Qwen3-0.6B Q8_0).

use super::schema::*;
use super::*;
use serde_json::json;

fn bytes(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap()
}

fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort();
    k
}

/// The example of llama.cpp's README (also the shape of Ollama's and LM Studio's examples).
fn systemone_example() -> Value {
    json!({
        "model": "decider",
        "state": "Customer message: I was charged twice for my order last week and nobody has replied.",
        "questions": {
            "route": {
                "type": "choice",
                "instructions": "Which team should handle this?",
                "criteria": {"billing": null, "shipping": "Delivery problems", "technical": null}
            },
            "angry": {"type": "noul", "instructions": "Is the customer angry?"},
            "urgency": {
                "type": "score",
                "instructions": "How urgent is this?",
                "criteria": ["can wait", "this week", "today", "right now"]
            }
        }
    })
}

/// vllm-d3-edge's answer to [`systemone_example`] (questions renamed `q0`..`q2` upstream).
fn native_answer() -> Value {
    json!({
        "model": "C:\\models\\vllm-d3-edge-Q8_0.gguf",
        "answers": {
            "q0": {"type": "choice", "choice": "billing",
                "probabilities": {"billing": 0.9388558756676372, "shipping": 0.03774105672502878, "technical": 0.02340306760733419},
                "confidence": 0.7535171271496977},
            "q1": {"type": "noul", "noul": 0.6105571606027178},
            "q2": {"type": "score", "score": 0.3621145060309049,
                "legend": {"0": "can wait", "1": "this week", "2": "today", "3": "right now"},
                "probabilities": {"0": 0.8313026971744737, "1": 0.06246530384466184, "2": 0.01904679475635027, "3": 0.08718520422451417},
                "confidence": 0.5563950740617253}
        },
        "usage": {"input_tokens": 329, "output_tokens": 0}
    })
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn systemone_request_reads_every_documented_field() {
    let mut body = systemone_example();
    body["images"] = json!(["iVBORw0KGgo=", "data:image/jpeg;base64,/9j/4AAQ"]);
    body["keep_alive"] = json!("5m");
    let req = parse_systemone(&bytes(body)).unwrap();
    assert_eq!(req.model, "decider");
    assert!(req.state.as_str().unwrap().starts_with("Customer message"));
    // Ollama sends bare base64, LM Studio and llama.cpp data URLs: both become data URLs.
    assert_eq!(
        req.images,
        vec![
            "data:image/png;base64,iVBORw0KGgo=",
            "data:image/jpeg;base64,/9j/4AAQ"
        ]
    );
    assert_eq!(req.keep_alive, Some(json!("5m")));
    let names: Vec<_> = req
        .questions
        .iter()
        .map(|q| q.name.clone().unwrap())
        .collect();
    assert_eq!(names, ["route", "angry", "urgency"]);
    let Kind::Choice(c) = &req.questions[0].kind else {
        panic!()
    };
    assert_eq!(c[1].key, "shipping");
    assert_eq!(c[1].description.as_deref(), Some("Delivery problems"));
    assert_eq!(c[0].description, None);
    assert_eq!(
        req.questions[1].kind,
        Kind::Noul {
            yes: None,
            no: None
        }
    );
    let Kind::Score(l) = &req.questions[2].kind else {
        panic!()
    };
    assert_eq!(l.len(), 4);
    // State and instructions may also be objects or arrays.
    let mut body = systemone_example();
    body["state"] = json!({"ticket": 7, "text": "refund"});
    body["questions"]["angry"]["instructions"] = json!(["Is the customer angry?"]);
    body["questions"]["angry"]["criteria"] = json!({"true": "Angry", "false": "Calm"});
    let req = parse_systemone(&bytes(body)).unwrap();
    assert!(req.state.is_object());
    assert_eq!(
        req.questions[1].kind,
        Kind::Noul {
            yes: Some("Angry".into()),
            no: Some("Calm".into())
        }
    );
}

#[test]
fn systemone_request_follows_ollamas_rules() {
    let refused = |edit: &dyn Fn(&mut Value), param: &str| {
        let mut body = systemone_example();
        edit(&mut body);
        let err = parse_systemone(&bytes(body)).unwrap_err();
        assert_eq!(err.param.as_deref(), Some(param), "{}", err.message);
    };
    refused(&|b| b["model"] = json!(" "), "model");
    refused(&|b| b["state"] = json!("   "), "state");
    refused(&|b| b["state"] = Value::Null, "state");
    refused(&|b| b["state"] = json!(3), "state");
    refused(&|b| b["questions"] = json!({}), "questions");
    refused(&|b| b["questions"] = json!([]), "questions");
    refused(
        &|b| {
            let many: serde_json::Map<String, Value> = (0..65)
                .map(|i| {
                    (
                        format!("q{i}"),
                        json!({"type": "noul", "instructions": "?"}),
                    )
                })
                .collect();
            b["questions"] = Value::Object(many);
        },
        "questions",
    );
    refused(
        &|b| b["questions"]["  "] = json!({"type": "noul", "instructions": "?"}),
        "questions",
    );
    refused(
        &|b| b["questions"]["route"]["criteria"] = json!({"billing": null}),
        "questions.route.criteria",
    );
    refused(
        &|b| b["questions"]["route"]["criteria"] = json!({"billing": 1, "x": null}),
        "questions.route.criteria",
    );
    refused(
        &|b| b["questions"]["angry"]["criteria"] = json!({"maybe": "?"}),
        "questions.angry.criteria",
    );
    refused(
        &|b| b["questions"]["urgency"]["criteria"] = json!(["only one"]),
        "questions.urgency.criteria",
    );
    refused(
        &|b| {
            b["questions"]["urgency"]["criteria"] =
                json!(["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10"])
        },
        "questions.urgency.criteria",
    );
    refused(
        &|b| b["questions"]["angry"]["type"] = json!("bool"),
        "questions.angry.type",
    );
    refused(
        &|b| b["questions"]["angry"]["instructions"] = json!(""),
        "questions.angry.instructions",
    );
    // URLs are not images (Ollama: "URLs and data URLs are not supported"; data URLs are
    // accepted here for LM Studio and llama.cpp).
    refused(
        &|b| b["images"] = json!(["https://example.com/a.png"]),
        "images[0]",
    );
    refused(&|b| b["stream"] = json!(true), "stream");
    // 255 options are allowed.
    let mut body = systemone_example();
    let many: serde_json::Map<String, Value> =
        (0..255).map(|i| (format!("o{i}"), Value::Null)).collect();
    body["questions"]["route"]["criteria"] = Value::Object(many);
    assert!(parse_systemone(&bytes(body)).is_ok());
}

#[test]
fn native_request_is_llama_servers_systemone() {
    let req = parse_systemone(&bytes(systemone_example())).unwrap();
    let up = native::request(&req, &[0, 1, 2]);
    assert_eq!(keys(&up), ["questions", "state"]);
    assert_eq!(
        up["questions"]["q0"],
        json!({"criteria": {"billing": null, "shipping": "Delivery problems", "technical": null},
            "type": "choice", "instructions": "Which team should handle this?"})
    );
    assert_eq!(
        up["questions"]["q1"],
        json!({"type": "noul", "instructions": "Is the customer angry?"})
    );
    assert_eq!(
        up["questions"]["q2"]["criteria"],
        json!(["can wait", "this week", "today", "right now"])
    );
    // Images go as llama.cpp's `files`.
    let mut with_image = req.clone();
    with_image.images = vec!["data:image/png;base64,AA==".into()];
    assert_eq!(
        native::request(&with_image, &[1])["files"],
        json!(["data:image/png;base64,AA=="])
    );
}

#[test]
fn systemone_response_has_ollamas_shape() {
    let req = parse_systemone(&bytes(systemone_example())).unwrap();
    let (answers, input, output) = native::answers(&req, &[0, 1, 2], &native_answer()).unwrap();
    let out = Outcome {
        answers,
        input_tokens: input,
        output_tokens: output,
    };
    let v = render_systemone("decider", &req, &out);
    assert_eq!(keys(&v), ["answers", "model", "usage"]);
    assert_eq!(v["model"], "decider");
    assert_eq!(v["usage"], json!({"input_tokens": 329, "output_tokens": 0}));
    let a = &v["answers"];
    assert_eq!(keys(a), ["angry", "route", "urgency"]);
    assert_eq!(
        keys(&a["route"]),
        ["choice", "confidence", "probabilities", "type"]
    );
    assert_eq!(a["route"]["choice"], "billing");
    // The same confidence as llama-server's (1 - H(p) / ln N).
    assert!(close(
        a["route"]["confidence"].as_f64().unwrap(),
        0.7535171271496977
    ));
    assert_eq!(keys(&a["angry"]), ["noul", "type"]);
    assert!(close(
        a["angry"]["noul"].as_f64().unwrap(),
        0.6105571606027178
    ));
    assert_eq!(
        keys(&a["urgency"]),
        ["confidence", "legend", "probabilities", "score", "type"]
    );
    assert!(close(
        a["urgency"]["score"].as_f64().unwrap(),
        0.3621145060309049
    ));
    assert!(close(
        a["urgency"]["confidence"].as_f64().unwrap(),
        0.5563950740617253
    ));
    assert_eq!(a["urgency"]["legend"]["3"], "right now");
    let sum: f64 = a["urgency"]["probabilities"]
        .as_object()
        .unwrap()
        .values()
        .map(|p| p.as_f64().unwrap())
        .sum();
    assert!(close(sum, 1.0));
}

#[test]
fn an_unanswered_question_is_an_error_not_a_guess() {
    let req = parse_systemone(&bytes(systemone_example())).unwrap();
    let mut body = native_answer();
    body["answers"].as_object_mut().unwrap().remove("q1");
    assert!(native::answers(&req, &[0, 1, 2], &body)
        .unwrap_err()
        .contains("angry"));
}

/// The OpenAI guide's examples: a predicate, a choice and a score question.
fn decisions_example() -> Value {
    json!({
        "model": "decider",
        "input": [{"role": "user", "content": [
            {"type": "input_text", "text": "The package arrived with a crushed corner."},
            {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo="}
        ]}],
        "questions": [
            {"type": "predicate", "name": "visible_damage", "instructions": "Is there visible damage?"},
            {"type": "choice", "name": "route", "instructions": "Which team?",
                "choices": [{"value": "billing", "description": "Payments"}, {"value": "shipping"}]},
            {"type": "score", "name": "severity", "instructions": "How severe?",
                "levels": [{"label": "minor"}, {"label": "moderate", "description": "Needs a refund"}, {"label": "severe"}]},
            {"type": "choice", "instructions": "Refund?", "choices": [{"value": true}, {"value": false}]}
        ]
    })
}

#[test]
fn decisions_request_reads_openais_shape() {
    let req = parse_decisions(&bytes(decisions_example())).unwrap();
    assert_eq!(
        req.state,
        json!("The package arrived with a crushed corner.")
    );
    assert_eq!(req.images, vec!["data:image/png;base64,iVBORw0KGgo="]);
    assert_eq!(req.questions.len(), 4);
    assert_eq!(
        req.questions[0].kind,
        Kind::Noul {
            yes: None,
            no: None
        }
    );
    assert_eq!(req.questions[3].name, None);
    let Kind::Choice(c) = &req.questions[3].kind else {
        panic!()
    };
    assert_eq!((c[0].key.as_str(), &c[0].value), ("true", &json!(true)));
    // To llama-server: predicate → noul, choices → criteria, levels → criteria texts.
    let up = native::request(&req, &[0, 1, 2]);
    assert_eq!(up["questions"]["q0"]["type"], "noul");
    assert_eq!(
        up["questions"]["q1"]["criteria"],
        json!({"billing": "Payments", "shipping": null})
    );
    assert_eq!(
        up["questions"]["q2"]["criteria"],
        json!(["minor", "moderate: Needs a refund", "severe"])
    );
    assert_eq!(up["files"], json!(["data:image/png;base64,iVBORw0KGgo="]));
    // A plain string input.
    let mut body = decisions_example();
    body["input"] = json!("I was charged twice.");
    assert!(parse_decisions(&bytes(body)).unwrap().images.is_empty());
}

#[test]
fn decisions_request_refusals_name_the_parameter() {
    let refused = |edit: &dyn Fn(&mut Value), param: &str| {
        let mut body = decisions_example();
        edit(&mut body);
        let err = parse_decisions(&bytes(body)).unwrap_err();
        assert_eq!(err.param.as_deref(), Some(param), "{}", err.message);
    };
    refused(&|b| b["input"] = json!(""), "input");
    refused(
        &|b| b["input"][0]["role"] = json!("assistant"),
        "input[0].role",
    );
    refused(
        &|b| b["input"][0]["content"][1]["image_url"] = json!("https://example.com/a.png"),
        "input[0].content[1].image_url",
    );
    refused(
        &|b| b["input"][0]["content"][1] = json!({"type": "input_image", "file_id": "file-1"}),
        "input[0].content[1].image_url",
    );
    refused(&|b| b["questions"] = json!([]), "questions");
    refused(
        &|b| b["questions"][0]["type"] = json!("noul"),
        "questions[0].type",
    );
    refused(
        &|b| b["questions"][1]["name"] = json!("visible_damage"),
        "questions[1].name",
    );
    refused(
        &|b| b["questions"][1]["choices"] = json!([]),
        "questions[1].choices",
    );
    refused(
        &|b| b["questions"][1]["choices"][1]["value"] = json!("billing"),
        "questions[1].choices[1].value",
    );
    refused(
        &|b| b["questions"][1]["choices"][1]["value"] = json!(3),
        "questions[1].choices[1].value",
    );
    refused(
        &|b| b["questions"][2]["levels"] = json!([{"label": "only"}]),
        "questions[2].levels",
    );
    refused(
        &|b| b["questions"][2]["levels"][0] = json!({"description": "x"}),
        "questions[2].levels[0].label",
    );
    refused(
        &|b| b["questions"][0]["instructions"] = json!(""),
        "questions[0].instructions",
    );
}

#[test]
fn decisions_response_has_openais_shape_and_no_undocumented_fields() {
    let req = parse_decisions(&bytes(decisions_example())).unwrap();
    let out = Outcome {
        answers: vec![
            Answer::Noul(0.92),
            Answer::Choice(vec![0.25, 0.75]),
            Answer::Score(vec![0.1, 0.7, 0.2]),
            Answer::Choice(vec![0.5, 0.5]),
        ],
        input_tokens: 40,
        output_tokens: 3,
    };
    let v = render_decisions("decider", &req, &out);
    // No `id`, `object` or `created_at`: OpenAI does not document them.
    assert_eq!(keys(&v), ["answers", "model", "usage"]);
    assert_eq!(
        v["usage"],
        json!({"input_tokens": 40, "output_tokens": 3, "total_tokens": 43})
    );
    let a = v["answers"].as_array().unwrap();
    // The guide's predicate example, field for field.
    assert_eq!(
        a[0],
        json!({"type": "predicate", "name": "visible_damage", "probability": 0.92})
    );
    assert_eq!(
        keys(&a[1]),
        ["choice", "confidence", "name", "probabilities", "type"]
    );
    assert_eq!(a[1]["choice"], "shipping");
    assert_eq!(
        a[1]["probabilities"],
        json!([{"value": "billing", "probability": 0.25}, {"value": "shipping", "probability": 0.75}])
    );
    // The guide's score example: 0 × 0.1 + 1 × 0.7 + 2 × 0.2 = 1.1.
    assert!(close(a[2]["score"].as_f64().unwrap(), 1.1));
    assert_eq!(
        a[2]["probabilities"][1],
        json!({"value": 1, "label": "moderate", "probability": 0.7})
    );
    // Unnamed questions answer with `name: null`; a tie goes to the first option; boolean
    // values come back as booleans.
    assert_eq!(a[3]["name"], Value::Null);
    assert_eq!(a[3]["choice"], json!(true));
    assert!(close(a[3]["confidence"].as_f64().unwrap(), 0.0));
}

#[test]
fn llama_server_501_becomes_the_400_clients_expect() {
    let body = br#"{"error":{"code":501,"message":"This model is not a decision model","type":"not_supported_error"}}"#;
    let err = upstream_error(501, body, "qwen3-0.6b");
    assert_eq!((err.status, err.code.as_str()), (400, "model_not_decision"));
    assert_eq!(err.param.as_deref(), Some("model"));
    let key = err.body()["error"]["harness"]["error_key"]
        .as_str()
        .unwrap()
        .to_string();
    let (key, params) = harness_core::error_key::parse(&key).unwrap();
    assert_eq!(key, ERR_DECISION_MODEL_REQUIRED);
    assert_eq!(params["model"], "qwen3-0.6b");
    // Ollama shape: `{"error": "<message>"}` with status 400.
    let resp = crate::ollama::error_response(err);
    assert_eq!(resp.status(), 400);
    // No image support (also a 501 upstream) is an invalid request with llama-server's text.
    let body = br#"{"error":{"code":501,"message":"This server does not support image input for decisions.","type":"not_supported_error"}}"#;
    let err = upstream_error(501, body, "m");
    assert_eq!((err.status, err.code.as_str()), (400, "invalid_request"));
    assert!(err.message.contains("image input"));
    let body = br#"{"error":{"code":400,"message":"questions.a: \"type\" must be one of: choice, score, noul","type":"invalid_request_error"}}"#;
    assert_eq!(upstream_error(400, body, "m").status, 400);
    assert_eq!(upstream_error(500, b"boom", "m").status, 500);
}

#[test]
fn emulation_asks_one_constrained_token_per_question() {
    let req = parse_systemone(&bytes(systemone_example())).unwrap();
    let r = emulate::request("qwen", &req.state, &[], &req.questions[0]);
    assert_eq!(r["grammar"], "root ::= \"A\" | \"B\" | \"C\"");
    assert_eq!(r["max_tokens"], 1);
    assert_eq!(r["top_logprobs"], 3);
    assert_eq!(r["post_sampling_probs"], true);
    let prompt = r["messages"][1]["content"].as_str().unwrap();
    assert!(prompt.starts_with("State:\nCustomer message"), "{prompt}");
    assert!(
        prompt.contains("B: shipping (Delivery problems)"),
        "{prompt}"
    );
    assert_eq!(
        emulate::request("qwen", &req.state, &[], &req.questions[1])["grammar"],
        "root ::= \"Y\" | \"N\""
    );
    assert_eq!(emulate::codes(&req.questions[2].kind), ["0", "1", "2", "3"]);
    // Images come before the text.
    let r = emulate::request(
        "qwen",
        &req.state,
        &["data:image/png;base64,AA==".into()],
        &req.questions[1],
    );
    assert_eq!(r["messages"][1]["content"][0]["type"], "image_url");
    // 52 one-character codes.
    let many = Kind::Choice(
        (0..52)
            .map(|i| Choice {
                key: format!("o{i}"),
                value: json!(format!("o{i}")),
                description: None,
            })
            .collect(),
    );
    let codes = emulate::codes(&many);
    assert_eq!((codes[25].as_str(), codes[51].as_str()), ("Z", "z"));
}

#[test]
fn emulation_reads_probabilities_after_the_grammar() {
    let codes: Vec<String> = ["A", "B", "C"].map(String::from).to_vec();
    // llama-server with `post_sampling_probs` (Qwen3-0.6B).
    let post = json!({"choices": [{"logprobs": {"content": [{"token": "C", "prob": 0.7400106191635132,
        "top_probs": [
            {"token": "C", "prob": 0.7400106191635132},
            {"token": "A", "prob": 0.2505705952644348},
            {"token": "B", "prob": 0.006920685991644859}]}]}}]});
    let p = emulate::probabilities(&post, &codes).unwrap();
    assert!(close(p.iter().sum::<f64>(), 1.0));
    assert!(p[2] > 0.73 && p[1] < 0.01);
    // Without it: the raw logprobs, renormalized over the codes (other tokens ignored).
    let raw = json!({"choices": [{"logprobs": {"content": [{"token": "C", "logprob": -0.27287667989730835,
        "top_logprobs": [
            {"token": "C", "logprob": -0.27287667989730835},
            {"token": "A", "logprob": -1.471710205078125},
            {"token": "B", "logprob": -5.053188323974609},
            {"token": "Answer", "logprob": -5.868858337402344}]}]}}]});
    let p = emulate::probabilities(&raw, &codes).unwrap();
    assert!(close(p.iter().sum::<f64>(), 1.0));
    assert!(p[2] > p[0] && p[0] > p[1]);
    // No logprobs (a server without them): no answer.
    assert!(emulate::probabilities(&json!({"choices": [{"message": {}}]}), &codes).is_none());
}

#[test]
fn confidence_matches_the_documented_formula() {
    assert!(close(confidence(&[0.25; 4]), 0.0));
    assert!(close(confidence(&[1.0, 0.0]), 1.0));
    assert!(close(
        confidence(&[0.9388558756676372, 0.03774105672502878, 0.02340306760733419]),
        0.7535171271496977
    ));
    assert_eq!(argmax(&[0.4, 0.4, 0.2]), 0);
    assert_eq!(normalize(vec![0.0, 0.0]), None);
}
