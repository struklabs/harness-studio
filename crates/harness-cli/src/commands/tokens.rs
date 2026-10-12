//! `api-token ...` (alias `token`): named Control API tokens, their scopes and their gateway
//! policy (allowed models and endpoints, rate limits). `otel ...`: OpenTelemetry export.

use harness_core::control_api::{
    CreatedToken, GatewayEndpoint, RateLimits, Scope, TokenInfo, TokenPolicy,
};
use serde_json::{json, Value};

use crate::cli::TokenPolicyArgs;
use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult};
use crate::output::{table, Out};

pub fn parse_scopes(raw: &[String]) -> CliResult<Vec<Scope>> {
    raw.iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| {
            Scope::parse(s).ok_or_else(|| {
                CliError::usage(
                    "cli.invalid_argument",
                    format!("Unknown scope \"{}\" ({})", s, Scope::names()),
                )
            })
        })
        .collect()
}

/// `--endpoint` values; `all` = every endpoint (no restriction).
pub fn parse_endpoints(raw: &[String]) -> CliResult<Vec<GatewayEndpoint>> {
    let mut out = Vec::new();
    for s in raw.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if s.eq_ignore_ascii_case("all") {
            return Ok(Vec::new());
        }
        let e = GatewayEndpoint::parse(s).ok_or_else(|| {
            CliError::usage(
                "cli.invalid_argument",
                format!(
                    "Unknown endpoint \"{}\" ({}, all)",
                    s,
                    GatewayEndpoint::ALL.map(|e| e.as_str()).join(", ")
                ),
            )
        })?;
        out.push(e);
    }
    Ok(out)
}

/// A limit flag: `0` removes the limit.
fn limit(v: Option<u32>) -> Option<u32> {
    v.filter(|n| *n > 0)
}

/// The limits after applying the flags given to `base` (flags not given keep `base`).
pub fn merged_limits(base: RateLimits, a: &TokenPolicyArgs) -> RateLimits {
    RateLimits {
        requests_per_minute: a.rpm.map_or(base.requests_per_minute, |v| limit(Some(v))),
        tokens_per_minute: a.tpm.map_or(base.tokens_per_minute, |v| limit(Some(v))),
        max_concurrent: a
            .concurrency
            .map_or(base.max_concurrent, |v| limit(Some(v))),
    }
}

fn limits_text(l: &RateLimits) -> String {
    let mut parts = Vec::new();
    if let Some(n) = l.requests_per_minute {
        parts.push(format!("{n} req/min"));
    }
    if let Some(n) = l.tokens_per_minute {
        parts.push(format!("{n} tok/min"));
    }
    if let Some(n) = l.max_concurrent {
        parts.push(format!("{n} at once"));
    }
    if parts.is_empty() {
        "-".into()
    } else {
        parts.join(", ")
    }
}

fn list_or_all(items: Vec<String>) -> String {
    if items.is_empty() {
        "all".into()
    } else {
        items.join(",")
    }
}

fn describe(t: &TokenInfo) -> String {
    format!(
        "{} ({})\n  scopes:    {}\n  models:    {}\n  endpoints: {}\n  limits:    {}",
        t.name,
        t.id,
        t.scopes
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(","),
        list_or_all(t.policy.models.clone()),
        list_or_all(
            t.policy
                .endpoints
                .iter()
                .map(|e| e.as_str().to_string())
                .collect()
        ),
        limits_text(&t.policy.limits)
    )
}

fn parse<T: serde::de::DeserializeOwned>(v: Value) -> CliResult<T> {
    serde_json::from_value(v)
        .map_err(|e| CliError::general("api.unexpected_response", e.to_string()))
}

pub async fn token_create(
    client: &ApiClient,
    name: &str,
    scopes: &[String],
    policy: &TokenPolicyArgs,
    out: &Out,
) -> CliResult<()> {
    let scopes = parse_scopes(scopes)?;
    let policy = TokenPolicy {
        models: policy.models.clone(),
        endpoints: parse_endpoints(&policy.endpoints)?,
        limits: merged_limits(RateLimits::default(), policy),
    };
    let mut body = json!({"name": name, "scopes": scopes});
    if let (Some(obj), Value::Object(p)) = (body.as_object_mut(), json!(policy)) {
        obj.extend(p);
    }
    let created: CreatedToken = parse(client.post("/tokens", &body).await?)?;
    let value = serde_json::to_value(&created).unwrap_or(Value::Null);
    out.data(&value, || {
        format!(
            "{}\n(token \"{}\", id {}; shown only once, store it now)\n{}",
            created.token,
            created.info.name,
            created.info.id,
            describe(&created.info)
        )
    });
    Ok(())
}

pub async fn token_ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let list: Vec<TokenInfo> = client.get_as("/tokens").await?;
    let value = serde_json::to_value(&list).unwrap_or(Value::Null);
    out.data(&value, || {
        if list.is_empty() {
            return "No named tokens.".into();
        }
        let rows: Vec<Vec<String>> = list
            .iter()
            .map(|t| {
                vec![
                    t.id.clone(),
                    t.name.clone(),
                    t.scopes
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(","),
                    list_or_all(t.policy.models.clone()),
                    list_or_all(
                        t.policy
                            .endpoints
                            .iter()
                            .map(|e| e.as_str().to_string())
                            .collect(),
                    ),
                    limits_text(&t.policy.limits),
                ]
            })
            .collect();
        table(
            &["ID", "NAME", "SCOPES", "MODELS", "ENDPOINTS", "LIMITS"],
            &rows,
        )
    });
    Ok(())
}

pub async fn token_show(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    let t: TokenInfo = client.get_as(&format!("/tokens/{}", seg(id))).await?;
    let value = serde_json::to_value(&t).unwrap_or(Value::Null);
    out.data(&value, || describe(&t));
    Ok(())
}

/// `api-token set`: only the flags given change (limits merge with the current ones).
pub async fn token_set(
    client: &ApiClient,
    id: &str,
    name: Option<&str>,
    scopes: &[String],
    policy: &TokenPolicyArgs,
    out: &Out,
) -> CliResult<()> {
    let mut change = serde_json::Map::new();
    if let Some(n) = name {
        change.insert("name".into(), json!(n));
    }
    if !scopes.is_empty() {
        change.insert("scopes".into(), json!(parse_scopes(scopes)?));
    }
    if !policy.models.is_empty() {
        // `--model '*'` = every model.
        change.insert("models".into(), json!(policy.models));
    }
    if !policy.endpoints.is_empty() {
        change.insert(
            "endpoints".into(),
            json!(parse_endpoints(&policy.endpoints)?),
        );
    }
    if policy.rpm.is_some() || policy.tpm.is_some() || policy.concurrency.is_some() {
        let current: TokenInfo = client.get_as(&format!("/tokens/{}", seg(id))).await?;
        change.insert(
            "limits".into(),
            json!(merged_limits(current.policy.limits, policy)),
        );
    }
    if change.is_empty() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "Nothing to change: give --name, --scope, --model, --endpoint, --rpm, --tpm or --concurrency.",
        ));
    }
    let updated: TokenInfo = parse(
        client
            .request(
                reqwest::Method::PATCH,
                &format!("/tokens/{}", seg(id)),
                Some(&Value::Object(change)),
            )
            .await?,
    )?;
    let value = serde_json::to_value(&updated).unwrap_or(Value::Null);
    out.data(&value, || describe(&updated));
    Ok(())
}

pub async fn token_rm(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    out.confirm(&format!("Revoke token {}?", id))?;
    client.delete(&format!("/tokens/{}", seg(id))).await?;
    let value = json!({"revoked": id});
    out.data(&value, || format!("Revoked {}", id));
    Ok(())
}

// --- OpenTelemetry ---

pub async fn otel_status(client: &ApiClient, out: &Out) -> CliResult<()> {
    let v = client.get("/otel").await?;
    out.data(&v, || {
        let s = &v["settings"];
        let st = &v["status"];
        let state = if st["active"].as_bool() == Some(true) {
            "on"
        } else if s["enabled"].as_bool() == Some(true) {
            "enabled, not running"
        } else {
            "off"
        };
        let mut lines = vec![
            format!("OpenTelemetry export: {}", state),
            format!(
                "  endpoint: {} ({})",
                s["endpoint"].as_str().unwrap_or("-"),
                s["protocol"].as_str().unwrap_or("-")
            ),
            format!(
                "  signals:  traces {}, logs {}, metrics {} (every {} s)",
                if s["traces"].as_bool() == Some(true) {
                    "on"
                } else {
                    "off"
                },
                if s["logs"].as_bool() == Some(true) {
                    "on"
                } else {
                    "off"
                },
                if s["metrics"].as_bool() == Some(true) {
                    "on"
                } else {
                    "off"
                },
                s["metrics_interval_secs"]
            ),
            format!(
                "  spans:    {} exported, {} dropped, {} export errors",
                st["spans_exported"], st["spans_dropped"], st["export_errors"]
            ),
            format!(
                "  logs:     {} exported, {} dropped",
                st["logs_exported"], st["logs_dropped"]
            ),
            format!(
                "  built-in: {}",
                if st["grpc"].as_bool() == Some(true) {
                    "http/protobuf, grpc"
                } else {
                    "http/protobuf (no gRPC in this build)"
                }
            ),
        ];
        if let Some(e) = st["last_error"].as_str() {
            lines.push(format!("  last error: {}", e));
        }
        lines.join("\n")
    });
    Ok(())
}

/// `otel test [--endpoint URL] [--protocol P]`: one real span to the (saved or given)
/// endpoint. Exit code 1 when the collector refuses it.
pub async fn otel_test(
    client: &ApiClient,
    endpoint: Option<&str>,
    protocol: Option<&str>,
    out: &Out,
) -> CliResult<()> {
    let mut body = json!({});
    if endpoint.is_some() || protocol.is_some() {
        let view = client.get("/otel").await?;
        let mut s = view["settings"].clone();
        if let Some(e) = endpoint {
            s["endpoint"] = json!(e);
        }
        if let Some(p) = protocol {
            s["protocol"] = json!(match p {
                "grpc" => "grpc",
                "http" | "http/protobuf" | "http_protobuf" => "http_protobuf",
                other => {
                    return Err(CliError::usage(
                        "cli.invalid_argument",
                        format!("Unknown protocol \"{}\" (http, grpc)", other),
                    ));
                }
            });
            // The instance says whether it was built with OTLP/gRPC (`otel-grpc`).
            if s["protocol"] == "grpc" && view["status"]["grpc"].as_bool() != Some(true) {
                return Err(CliError::usage(
                    harness_core::otel::ERR_GRPC_NOT_BUILT,
                    crate::messages::core_text(harness_core::otel::ERR_GRPC_NOT_BUILT),
                ));
            }
        }
        body["settings"] = s;
    }
    let r = client.post("/otel/test", &body).await?;
    out.data(&r, || {
        if r["ok"].as_bool() == Some(true) {
            format!(
                "Test span sent to {} in {} ms (trace id {}).",
                r["url"].as_str().unwrap_or("-"),
                r["elapsed_ms"],
                r["trace_id"].as_str().unwrap_or("-")
            )
        } else {
            format!(
                "Test export to {} failed: {}",
                r["url"].as_str().unwrap_or("-"),
                r["error"].as_str().unwrap_or("-")
            )
        }
    });
    if r["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(CliError::general(
            "otel.test_failed",
            r["error"].as_str().unwrap_or("export failed").to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Exit;

    #[test]
    fn scopes_parse_and_reject_unknown() {
        let s = parse_scopes(&["read".into(), "Models:Write".into()]).unwrap();
        assert_eq!(s, vec![Scope::Read, Scope::ModelsWrite]);
        let s = parse_scopes(&["servers:control".into(), "downloads:write".into()]).unwrap();
        assert_eq!(s, vec![Scope::ServersControl, Scope::DownloadsWrite]);
        assert_eq!(
            parse_scopes(&["root".into()]).unwrap_err().exit,
            Exit::Usage
        );
    }

    #[test]
    fn endpoints_and_limits_parse() {
        assert_eq!(
            parse_endpoints(&["chat".into(), "Ollama".into()]).unwrap(),
            vec![GatewayEndpoint::Chat, GatewayEndpoint::Ollama]
        );
        assert!(parse_endpoints(&["chat".into(), "all".into()])
            .unwrap()
            .is_empty());
        assert_eq!(
            parse_endpoints(&["files".into()]).unwrap_err().exit,
            Exit::Usage
        );
        let base = RateLimits {
            requests_per_minute: Some(10),
            tokens_per_minute: Some(1000),
            max_concurrent: None,
        };
        let args = TokenPolicyArgs {
            rpm: Some(0),
            concurrency: Some(2),
            ..Default::default()
        };
        assert_eq!(
            merged_limits(base, &args),
            RateLimits {
                requests_per_minute: None,
                tokens_per_minute: Some(1000),
                max_concurrent: Some(2),
            }
        );
        assert_eq!(limits_text(&RateLimits::default()), "-");
        assert_eq!(
            limits_text(&merged_limits(base, &args)),
            "1000 tok/min, 2 at once"
        );
    }
}
