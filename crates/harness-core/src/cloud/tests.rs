use super::*;
use serde_json::json;

#[test]
fn model_ids_round_trip() {
    assert_eq!(model_id("openai", "gpt-4o"), "cloud:openai/gpt-4o");
    assert_eq!(
        parse_model("cloud:router/meta-llama/llama-3-8b"),
        Some(("router", "meta-llama/llama-3-8b"))
    );
    assert_eq!(parse_model("cloud:Bad/x"), None);
    assert_eq!(parse_model("cloud:openai/"), None);
    assert_eq!(parse_model("remote:openai/x"), None);
}

#[test]
fn urls_need_https_unless_local() {
    assert_eq!(
        validate_url("", Kind::Openai).unwrap(),
        "https://api.openai.com/v1"
    );
    assert!(validate_url("", Kind::Custom).is_err());
    assert_eq!(
        validate_url("http://127.0.0.1:8080", Kind::Custom).unwrap(),
        "http://127.0.0.1:8080/v1"
    );
    assert_eq!(
        validate_url("http://localhost:1234/api/v1/", Kind::Custom).unwrap(),
        "http://localhost:1234/api/v1"
    );
    assert_eq!(
        validate_url("http://192.168.1.5:8000/v1", Kind::Custom).unwrap(),
        "http://192.168.1.5:8000/v1"
    );
    for bad in [
        "http://example.com/v1",
        "http://8.8.8.8/v1",
        "ftp://host/v1",
        "https://host/v1?key=x",
        "https://user:pass@host/v1",
        "not a url",
    ] {
        assert!(validate_url(bad, Kind::Custom).is_err(), "{bad}");
    }
}

#[test]
fn openrouter_models_carry_price_context_and_capabilities() {
    let body = json!({"data": [{
        "id": "openai/gpt-4o-mini",
        "name": "OpenAI: GPT-4o-mini",
        "context_length": 128000,
        "pricing": {"prompt": "0.00000015", "completion": "0.0000006"},
        "supported_parameters": ["tools", "temperature"],
        "architecture": {"input_modalities": ["text", "image"]}
    }, {
        "id": "x/no-tools",
        "supported_parameters": ["temperature"],
        "architecture": {"input_modalities": ["text"]}
    }]});
    let models = parse_models(Kind::Openrouter, &body).unwrap();
    assert_eq!(models.len(), 2);
    let m = &models[0];
    assert_eq!(m.id, "openai/gpt-4o-mini");
    assert_eq!(m.display_name.as_deref(), Some("OpenAI: GPT-4o-mini"));
    assert_eq!(m.context_length, Some(128000));
    assert_eq!(m.input_price, Some(0.15));
    assert_eq!(m.output_price, Some(0.6));
    assert_eq!(m.tools, Some(true));
    assert_eq!(m.vision, Some(true));
    assert_eq!(models[1].tools, Some(false));
    assert_eq!(models[1].vision, Some(false));
}

#[test]
fn provider_lists_are_filtered_and_normalized() {
    let openai = json!({"data": [
        {"id": "gpt-4o", "owned_by": "openai"},
        {"id": "text-embedding-3-small"},
        {"id": "whisper-1"},
        {"id": "gpt-4o"}
    ]});
    let models = parse_models(Kind::Openai, &openai).unwrap();
    let ids: Vec<_> = models
        .iter()
        .map(|m| (m.id.as_str(), m.embedding))
        .collect();
    assert_eq!(ids, [("gpt-4o", false), ("text-embedding-3-small", true)]);
    let gemini = json!({"data": [
        {"id": "models/gemini-2.5-flash", "object": "model"},
        {"id": "models/text-embedding-004"}
    ]});
    let models = parse_models(Kind::Gemini, &gemini).unwrap();
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "gemini-2.5-flash");
    assert!(!models[0].embedding);
    assert_eq!(models[1].id, "text-embedding-004");
    assert!(models[1].embedding);
    let anthropic = json!({"data": [
        {"id": "claude-x", "display_name": "Claude X", "type": "model"}
    ], "has_more": false});
    let models = parse_models(Kind::Anthropic, &anthropic).unwrap();
    assert_eq!(models[0].display_name.as_deref(), Some("Claude X"));
    assert_eq!(models[0].tools, None);
    assert!(parse_models(Kind::Custom, &json!({"error": "x"})).is_err());
}

#[test]
fn quota_is_read_from_the_key_endpoint() {
    let q = parse_quota(&json!({"data": {"limit": 10.0, "usage": 2.5, "limit_remaining": 7.5}}))
        .unwrap();
    assert_eq!(q.limit, Some(10.0));
    assert_eq!(q.usage, Some(2.5));
    assert_eq!(q.remaining, Some(7.5));
    assert!(parse_quota(&json!({"data": {"label": "x"}})).is_none());
}

#[test]
fn status_errors_are_keys() {
    assert_eq!(status_error(401), "cloud.unauthorized");
    assert_eq!(status_error(403), "cloud.unauthorized");
    assert_eq!(status_error(429), "cloud.rate_limited");
    assert_eq!(status_error(402), "cloud.payment_required");
    assert_eq!(status_error(500), "cloud.server_error");
    assert_eq!(status_error(502), "cloud.server_error");
    assert_eq!(status_error(418), "cloud.unreachable");
}

#[test]
fn rewrite_uses_the_provider_model_id() {
    let body = serde_json::to_vec(&json!({"model": "cloud:router/a/b", "stream": true})).unwrap();
    let out: Value =
        serde_json::from_slice(&rewrite_model("cloud:router/a/b", &body).unwrap()).unwrap();
    assert_eq!(out["model"], "a/b");
    assert_eq!(out["stream"], true);
}

#[test]
fn model_view_keeps_the_gateway_id() {
    let view = ModelView {
        id: model_id("router", "a/b"),
        model: "a/b".into(),
        provider: "router".into(),
        kind: Kind::Openrouter,
        label: "OpenRouter",
        state: State::Online,
        info: CloudModel {
            id: "a/b".into(),
            context_length: Some(8),
            ..Default::default()
        },
    };
    let json = serde_json::to_value(&view).unwrap();
    assert_eq!(json["id"], "cloud:router/a/b");
    assert_eq!(json["model"], "a/b");
    assert_eq!(json["context_length"], 8);
}

/// "Test Connection" lists what it found by name: `CloudModel` alone does not serialize its id.
#[test]
fn probe_models_carry_the_provider_model_id() {
    let found = ProbeModel {
        model: "qwen3-byok".into(),
        info: CloudModel {
            id: "qwen3-byok".into(),
            tools: Some(true),
            ..Default::default()
        },
    };
    let json = serde_json::to_value(&found).unwrap();
    assert_eq!(json["model"], "qwen3-byok");
    assert_eq!(json["tools"], true);
    assert!(json.get("id").is_none());
}

#[test]
fn providers_accept_only_the_protocols_they_speak() {
    for kind in [
        Kind::Openai,
        Kind::Gemini,
        Kind::Anthropic,
        Kind::Openrouter,
        Kind::Custom,
        Kind::CustomAnthropic,
    ] {
        assert!(kind.accepts("/v1/chat/completions"));
        assert!(!kind.accepts("/v1/completions"));
    }
    assert!(Kind::Openai.accepts("/v1/responses"));
    assert!(!Kind::Gemini.accepts("/v1/responses"));
    assert!(!Kind::Anthropic.accepts("/v1/responses"));
    assert!(Kind::Anthropic.accepts("/v1/messages"));
    assert!(Kind::Anthropic.accepts("/v1/messages/count_tokens"));
    assert!(!Kind::Openai.accepts("/v1/messages"));
    assert!(Kind::Gemini.accepts("/v1/embeddings"));
    assert!(!Kind::Anthropic.accepts("/v1/embeddings"));
    assert!(Kind::CustomAnthropic.accepts("/v1/messages"));
    assert!(!Kind::CustomAnthropic.accepts("/v1/responses"));
    assert!(!Kind::CustomAnthropic.accepts("/v1/embeddings"));
    assert!(!Kind::CustomAnthropic.key_required() && Kind::CustomAnthropic.anthropic_wire());
    assert_eq!(
        validate_url("http://127.0.0.1:9000", Kind::CustomAnthropic).unwrap(),
        "http://127.0.0.1:9000/v1"
    );
    assert_eq!(
        serde_json::to_string(&Kind::CustomAnthropic).unwrap(),
        "\"custom_anthropic\""
    );
}

#[test]
fn the_key_is_not_in_the_debug_output_of_an_input() {
    let input: ProviderInput =
        serde_json::from_value(json!({"name": "p", "kind": "openai", "key": "sk-secret-value"}))
            .unwrap();
    assert!(!format!("{input:?}").contains("sk-secret-value"));
}

#[test]
fn capabilities_come_from_what_each_api_lists() {
    let anthropic = json!({"data": [{
        "id": "claude-x", "display_name": "Claude X", "max_input_tokens": 200000,
        "capabilities": {"image_input": {"supported": true}, "thinking": {"supported": false}}
    }]});
    let m = &parse_models(Kind::Anthropic, &anthropic).unwrap()[0];
    assert_eq!(
        (m.context_length, m.vision, m.reasoning),
        (Some(200000), Some(true), Some(false))
    );
    // A custom Anthropic-compatible endpoint is read the same way.
    let c = &parse_models(Kind::CustomAnthropic, &anthropic).unwrap()[0];
    assert_eq!(c.vision, Some(true));
    let router = json!({"data": [
        {"id": "a/think", "supported_parameters": ["tools", "reasoning"]},
        {"id": "a/plain", "supported_parameters": ["tools"]},
        {"id": "a/unknown"}
    ]});
    let models = parse_models(Kind::Openrouter, &router).unwrap();
    let reasoning: Vec<_> = models
        .iter()
        .map(|m| (m.id.as_str(), m.reasoning))
        .collect();
    assert_eq!(
        reasoning,
        [
            ("a/plain", Some(false)),
            ("a/think", Some(true)),
            ("a/unknown", None)
        ]
    );
    // OpenAI-style lists say nothing: the tags stay unknown rather than guessed.
    let openai = json!({"data": [{"id": "gpt-x"}]});
    let m = &parse_models(Kind::Openai, &openai).unwrap()[0];
    assert_eq!((m.tools, m.vision, m.reasoning), (None, None, None));
}

#[test]
fn provider_errors_carry_a_retry_hint() {
    let key = |e: String| {
        crate::error_key::parse(&e)
            .map(|(k, p)| (k.to_string(), p))
            .unwrap()
    };
    assert_eq!(key(http_error(401, None, b"")).0, "cloud.unauthorized");
    assert_eq!(key(http_error(403, None, b"")).0, "cloud.unauthorized");
    assert_eq!(key(http_error(402, None, b"")).0, "cloud.payment_required");
    assert_eq!(
        key(http_error(404, None, b"")).0,
        "cloud.not_found_upstream"
    );
    assert_eq!(key(http_error(429, None, b"{}")).0, "cloud.rate_limited");
    let (k, params) = key(http_error(429, Some(7), b"{}"));
    assert_eq!(
        (k.as_str(), params["retry_after"].as_u64()),
        ("cloud.rate_limited_retry", Some(7))
    );
    // Out of credit is not a rate limit: waiting does not help.
    let quota =
        br#"{"error":{"code":"insufficient_quota","message":"You exceeded your current quota"}}"#;
    assert_eq!(
        key(http_error(429, Some(7), quota)).0,
        "cloud.quota_exceeded"
    );
    assert_eq!(key(http_error(503, None, b"")).0, "cloud.server_error");
    assert_eq!(
        key(http_error(529, Some(30), b"")).0,
        "cloud.server_error_retry"
    );
    let mut h = reqwest::header::HeaderMap::new();
    h.insert("retry-after", "12".parse().unwrap());
    assert_eq!(retry_after_secs(&h), Some(12));
    h.insert(
        "retry-after",
        "Wed, 21 Oct 2026 07:28:00 GMT".parse().unwrap(),
    );
    assert_eq!(retry_after_secs(&h), None);
}

#[test]
fn extra_headers_are_checked() {
    let h = |n: &str, v: &str| HeaderInput {
        name: n.into(),
        value: v.into(),
    };
    assert_eq!(
        check_headers(&[h("X-Org-Id", " abc ")]).unwrap(),
        [("x-org-id".to_string(), "abc".to_string())]
    );
    for bad in [
        h("Authorization", "x"),
        h("x-api-key", "x"),
        h("Host", "evil"),
        h("x-harness-link-hop", "1"),
        h("bad name", "x"),
        h("x-ok", "line\nbreak"),
        h("x-ok", ""),
    ] {
        assert!(check_headers(&[bad]).is_err());
    }
    assert!(check_headers(&[h("x-a", "1"), h("X-A", "2")]).is_err());
}

#[test]
fn costs_use_the_users_price_over_the_providers() {
    // 1000 prompt + 500 completion tokens at 2 / 8 USD per million.
    assert_eq!(
        cost_of(Some(2.0), Some(8.0), Some(1000), Some(500)),
        Some(0.006)
    );
    // Only one side priced: the other counts as free.
    assert_eq!(
        cost_of(Some(2.0), None, Some(1_000_000), Some(5)),
        Some(2.0)
    );
    assert_eq!(cost_of(None, None, Some(10), Some(10)), None);
    assert_eq!(cost_of(Some(1.0), Some(1.0), None, None), None);
    let api = CloudModel {
        id: "m".into(),
        input_price: Some(1.0),
        output_price: Some(2.0),
        ..Default::default()
    };
    let custom = ModelPrice {
        input: None,
        output: Some(5.0),
    };
    let m = priced(&api, Some(&custom));
    assert_eq!(
        (m.input_price, m.output_price, m.price_custom),
        (Some(1.0), Some(5.0), true)
    );
    assert!(!priced(&api, None).price_custom);
    assert!(!ModelPrice {
        input: Some(-1.0),
        output: None
    }
    .valid());
    assert!(!ModelPrice {
        input: Some(f64::NAN),
        output: None
    }
    .valid());
}

#[test]
fn a_connection_check_names_each_model_it_found() {
    // `CloudModel` leaves its id out; without `model` the add dialog had no name or key per row.
    let found = ProbeModel {
        model: "gpt-x".into(),
        info: CloudModel {
            id: "gpt-x".into(),
            tools: Some(true),
            ..Default::default()
        },
    };
    let json = serde_json::to_value(&found).unwrap();
    assert_eq!(json["model"], "gpt-x");
    assert_eq!(json["tools"], true);
    assert!(json.get("id").is_none());
}
