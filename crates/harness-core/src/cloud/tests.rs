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
    let ids: Vec<_> = parse_models(Kind::Openai, &openai)
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(ids, ["gpt-4o"]);
    let gemini = json!({"data": [
        {"id": "models/gemini-2.5-flash", "object": "model"},
        {"id": "models/text-embedding-004"}
    ]});
    let models = parse_models(Kind::Gemini, &gemini).unwrap();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "gemini-2.5-flash");
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
    assert_eq!(status_error(500), "cloud.unreachable");
}

#[test]
fn rewrite_uses_the_provider_model_id() {
    let body = serde_json::to_vec(&json!({"model": "cloud:router/a/b", "stream": true})).unwrap();
    let out: Value =
        serde_json::from_slice(&rewrite_model("cloud:router/a/b", &body).unwrap()).unwrap();
    assert_eq!(out["model"], "a/b");
    assert_eq!(out["stream"], true);
}
