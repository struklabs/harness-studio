//! JSON-RPC dispatch for both protocol eras.
//!
//! * **Modern** (2026-07-28): every request carries `params._meta` with
//!   `io.modelcontextprotocol/protocolVersion` and `.../clientCapabilities`; there is no
//!   handshake; results carry `resultType: "complete"` (and caching hints on
//!   `server/discover` / `tools/list`). An unknown version gets `-32022` with the supported
//!   list, missing metadata `-32602`.
//! * **Legacy** (2025-11-25 and earlier): `initialize` negotiates a version; later requests
//!   carry no `_meta` version. The server keeps no session state, so a legacy request is
//!   served the same way whether or not `initialize` came first.
//!
//! Over HTTP the transport adds header checks ([`check_http_headers`]) and maps the
//! [`Reply::status`] hint to the response status.

use serde_json::{json, Map, Value};

use crate::tools;
use crate::McpServer;

/// Revisions served statelessly with per-request `_meta`.
pub const MODERN_VERSIONS: [&str; 1] = ["2026-07-28"];
/// `initialize`-based revisions, newest first.
pub const LEGACY_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
pub const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
pub const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
/// Client name in a stateless request, when the client sends one (stdio and `initialize`
/// clients are named by the handshake instead).
pub const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const HEADER_MISMATCH: i64 = -32020;
pub const UNSUPPORTED_VERSION: i64 = -32022;

/// How long clients may cache `tools/list` / `server/discover` (5 min). The list depends on
/// the caller's token, hence `cacheScope: private`.
const TTL_MS: u64 = 300_000;

/// Which protocol era a request belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Era {
    Modern(String),
    Legacy,
}

/// A JSON-RPC response plus the HTTP status the streamable HTTP transport should use.
#[derive(Debug, Clone)]
pub struct Reply {
    pub body: Value,
    pub status: u16,
}

impl Reply {
    fn ok(id: &Value, result: Value) -> Self {
        Reply {
            body: json!({"jsonrpc": "2.0", "id": id, "result": result}),
            status: 200,
        }
    }
}

/// A JSON-RPC error response.
pub fn error_reply(
    id: &Value,
    code: i64,
    message: &str,
    data: Option<Value>,
    status: u16,
) -> Reply {
    let mut error = json!({"code": code, "message": message});
    if let Some(d) = data {
        error["data"] = d;
    }
    Reply {
        body: json!({"jsonrpc": "2.0", "id": id, "error": error}),
        status,
    }
}

fn unsupported_version(id: &Value, requested: &str) -> Reply {
    let supported: Vec<&str> = MODERN_VERSIONS
        .iter()
        .chain(LEGACY_VERSIONS.iter())
        .copied()
        .collect();
    error_reply(
        id,
        UNSUPPORTED_VERSION,
        "Unsupported protocol version",
        Some(json!({"supported": supported, "requested": requested})),
        400,
    )
}

fn meta(msg: &Value) -> Option<&Map<String, Value>> {
    msg.get("params")?.get("_meta")?.as_object()
}

fn server_info() -> Value {
    json!({
        "name": crate::SERVER_NAME,
        "title": "Harness Studio",
        "version": crate::SERVER_VERSION,
    })
}

/// Guidance for the model (`instructions`).
pub const INSTRUCTIONS: &str = "Harness Studio manages local llama.cpp models (GGUF) and \
llama-server instances on this machine. Typical flow: list_models or recommend_models → \
load_model (waits until the server is ready) → get_endpoint (OpenAI-compatible base URL and \
model id to send requests to) → unload_model when done. hardware_status shows GPUs and free \
VRAM; get_metrics/get_logs inspect a running server; run_benchmark and compare_performance \
measure speed. Model arguments accept a definition alias, a library model name/file name or \
an absolute .gguf path. Errors carry a machine code (e.g. servers.out_of_memory) and, after \
an out-of-memory failure, recovery options.";

fn capabilities() -> Value {
    json!({"tools": {"listChanged": false}})
}

/// Headers of a streamable HTTP request that the transport mirrors from the body.
#[derive(Debug, Clone, Default)]
pub struct HttpHeaders {
    pub protocol_version: Option<String>,
    pub method: Option<String>,
    pub name: Option<String>,
}

/// `=?base64?...?=` sentinel values of `Mcp-Name` / `Mcp-Param-*` headers.
fn decode_header_value(raw: &str) -> Option<String> {
    match raw
        .strip_prefix("=?base64?")
        .and_then(|r| r.strip_suffix("?="))
    {
        Some(b64) => String::from_utf8(base64_decode(b64)?).ok(),
        None => Some(raw.to_string()),
    }
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in input.bytes().filter(|c| *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Streamable HTTP header rules. A modern `MCP-Protocol-Version` needs `Mcp-Method` (and
/// `Mcp-Name` for `tools/call`) matching the body, and the same version in `_meta`
/// (`-32020`, 400). An unknown version is `-32022` (400). No header or a legacy version:
/// legacy request (a missing header counts as 2025-03-26).
pub fn check_http_headers(h: &HttpHeaders, msg: &Value) -> Result<(), Reply> {
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let Some(version) = h.protocol_version.as_deref() else {
        return Ok(());
    };
    if LEGACY_VERSIONS.contains(&version) {
        return Ok(());
    }
    if !MODERN_VERSIONS.contains(&version) {
        return Err(unsupported_version(&id, version));
    }
    let mismatch = |what: String| Err(error_reply(&id, HEADER_MISMATCH, &what, None, 400));
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    match h.method.as_deref() {
        None => return mismatch("Header mismatch: Mcp-Method header is missing".into()),
        Some(m) if m != method => {
            return mismatch(format!(
                "Header mismatch: Mcp-Method header value '{}' does not match body value '{}'",
                m, method
            ))
        }
        _ => {}
    }
    let body_version = meta(msg)
        .and_then(|m| m.get(META_VERSION))
        .and_then(Value::as_str);
    if msg.get("id").is_some() && body_version != Some(version) {
        return mismatch(format!(
            "Header mismatch: MCP-Protocol-Version header value '{}' does not match body value '{}'",
            version,
            body_version.unwrap_or("")
        ));
    }
    if matches!(method, "tools/call" | "resources/read" | "prompts/get") {
        let params = msg.get("params");
        let body_name = params
            .and_then(|p| p.get("name").or_else(|| p.get("uri")))
            .and_then(Value::as_str)
            .unwrap_or("");
        match h.name.as_deref().map(decode_header_value) {
            None => return mismatch("Header mismatch: Mcp-Name header is missing".into()),
            Some(None) => return mismatch("Header mismatch: Mcp-Name header is malformed".into()),
            Some(Some(n)) if n != body_name => {
                return mismatch(format!(
                    "Header mismatch: Mcp-Name header value '{}' does not match body value '{}'",
                    n, body_name
                ))
            }
            _ => {}
        }
    }
    Ok(())
}

impl McpServer {
    /// Handles one JSON-RPC message. `None` for notifications and client responses (nothing
    /// to send back; HTTP answers 202).
    pub async fn handle(&self, msg: &Value) -> Option<Reply> {
        let Some(obj) = msg.as_object() else {
            return Some(error_reply(
                &Value::Null,
                INVALID_REQUEST,
                "Invalid request: expected a JSON-RPC object (batches are not supported)",
                None,
                400,
            ));
        };
        let id = obj.get("id").cloned();
        let Some(method) = obj.get("method").and_then(Value::as_str) else {
            if obj.contains_key("result") || obj.contains_key("error") {
                return None; // a response from the client: this server sends no requests
            }
            return Some(error_reply(
                &id.unwrap_or(Value::Null),
                INVALID_REQUEST,
                "Invalid request: no method",
                None,
                400,
            ));
        };
        // Notifications (`notifications/initialized`, `notifications/cancelled`...): no reply.
        let id = id?;
        if id.is_null() {
            return Some(error_reply(
                &id,
                INVALID_REQUEST,
                "Invalid request: id must not be null",
                None,
                400,
            ));
        }
        let era = match self.era(msg, method) {
            Ok(era) => era,
            Err(reply) => return Some(reply),
        };
        let modern = matches!(era, Era::Modern(_));
        let reply = match self.dispatch(&id, method, msg, &era).await {
            Ok(mut result) => {
                if modern {
                    if let Some(r) = result.as_object_mut() {
                        r.insert("resultType".into(), json!("complete"));
                        let meta = r.entry("_meta").or_insert_with(|| json!({}));
                        if let Some(m) = meta.as_object_mut() {
                            m.insert(META_SERVER_INFO.into(), server_info());
                        }
                    }
                }
                Reply::ok(&id, result)
            }
            Err(mut reply) => {
                // Legacy clients read JSON-RPC errors from a 200 response.
                if !modern && reply.status != 400 {
                    reply.status = 200;
                }
                reply
            }
        };
        Some(reply)
    }

    fn era(&self, msg: &Value, method: &str) -> Result<Era, Reply> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        if method == "initialize" {
            return Ok(Era::Legacy);
        }
        let Some(meta) = meta(msg) else {
            return Ok(Era::Legacy);
        };
        let Some(version) = meta.get(META_VERSION) else {
            return Ok(Era::Legacy);
        };
        let Some(version) = version.as_str() else {
            return Err(error_reply(
                &id,
                INVALID_PARAMS,
                "Invalid params: _meta protocolVersion must be a string",
                None,
                400,
            ));
        };
        if LEGACY_VERSIONS.contains(&version) {
            return Ok(Era::Legacy);
        }
        if !MODERN_VERSIONS.contains(&version) {
            return Err(unsupported_version(&id, version));
        }
        if !meta.get(META_CAPABILITIES).is_some_and(Value::is_object) {
            return Err(error_reply(
                &id,
                INVALID_PARAMS,
                "Invalid params: _meta is missing io.modelcontextprotocol/clientCapabilities",
                None,
                400,
            ));
        }
        Ok(Era::Modern(version.to_string()))
    }

    async fn dispatch(
        &self,
        id: &Value,
        method: &str,
        msg: &Value,
        era: &Era,
    ) -> Result<Value, Reply> {
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));
        let modern = matches!(era, Era::Modern(_));
        match method {
            "initialize" => {
                if let Some(name) = params["clientInfo"]["name"].as_str() {
                    self.set_client_name(name);
                }
                let requested = params["protocolVersion"].as_str().unwrap_or("");
                let version = if LEGACY_VERSIONS.contains(&requested) {
                    requested
                } else {
                    LEGACY_VERSIONS[0]
                };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": capabilities(),
                    "serverInfo": server_info(),
                    "instructions": INSTRUCTIONS,
                }))
            }
            "server/discover" => {
                if !modern {
                    return Err(error_reply(
                        id,
                        INVALID_PARAMS,
                        "Invalid params: server/discover needs _meta with io.modelcontextprotocol/protocolVersion",
                        None,
                        400,
                    ));
                }
                let supported: Vec<&str> = MODERN_VERSIONS
                    .iter()
                    .chain(LEGACY_VERSIONS.iter())
                    .copied()
                    .collect();
                Ok(json!({
                    "supportedVersions": supported,
                    "capabilities": capabilities(),
                    "instructions": INSTRUCTIONS,
                    "ttlMs": TTL_MS,
                    "cacheScope": "public",
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let mut tools: Vec<Value> = tools::catalog()
                    .iter()
                    .filter(|t| self.allows(t.scope))
                    .map(|t| t.to_json())
                    .collect();
                tools.extend(crate::harness_tools::list(self).await);
                let mut result = json!({"tools": tools});
                if modern {
                    result["ttlMs"] = json!(TTL_MS);
                    result["cacheScope"] = json!("private");
                }
                Ok(result)
            }
            "tools/call" => {
                let Some(name) = params["name"].as_str() else {
                    return Err(error_reply(
                        id,
                        INVALID_PARAMS,
                        "Invalid params: tools/call needs a tool name",
                        None,
                        400,
                    ));
                };
                let args = match &params["arguments"] {
                    Value::Null => json!({}),
                    Value::Object(_) => params["arguments"].clone(),
                    _ => {
                        return Ok(tools::error_result(
                            "invalid_arguments",
                            "arguments must be an object",
                            None,
                        ))
                    }
                };
                let Some(def) = tools::find(name) else {
                    let meta_client = params["_meta"][META_CLIENT_INFO]["name"].as_str();
                    return match crate::harness_tools::call(self, name, &args, meta_client).await {
                        Some(result) => Ok(result),
                        None => Err(error_reply(
                            id,
                            INVALID_PARAMS,
                            &format!("Unknown tool: {}", name),
                            None,
                            200,
                        )),
                    };
                };
                if !self.allows(def.scope) {
                    return Ok(tools::error_result(
                        "api.insufficient_scope",
                        &format!(
                            "{} needs the \"{}\" scope, which this connection does not have ({}).",
                            name,
                            def.scope.as_str(),
                            match self.transport {
                                crate::Transport::Stdio =>
                                    "start `harness mcp` with --scope to allow it",
                                crate::Transport::Http => "use a token with that scope",
                            }
                        ),
                        Some(json!({"scope": def.scope.as_str()})),
                    ));
                }
                Ok(tools::call(self, name, &args).await)
            }
            other => Err(error_reply(
                id,
                METHOD_NOT_FOUND,
                &format!("Method not found: {}", other),
                None,
                404,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_sentinels_decode() {
        assert_eq!(
            decode_header_value("=?base64?SGVsbG8sIOS4lueVjA==?=").as_deref(),
            Some("Hello, 世界")
        );
        assert_eq!(
            decode_header_value("load_model").as_deref(),
            Some("load_model")
        );
        assert_eq!(decode_header_value("=?base64?!!?="), None);
    }

    fn modern_call(version: &str, name: &str) -> Value {
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": name, "arguments": {},
            "_meta": {META_VERSION: version, META_CAPABILITIES: {}}}})
    }

    #[test]
    fn http_headers_must_mirror_the_body() {
        let msg = modern_call("2026-07-28", "list_models");
        let ok = HttpHeaders {
            protocol_version: Some("2026-07-28".into()),
            method: Some("tools/call".into()),
            name: Some("list_models".into()),
        };
        assert!(check_http_headers(&ok, &msg).is_ok());
        let wrong_name = HttpHeaders {
            name: Some("unload_model".into()),
            ..ok.clone()
        };
        let e = check_http_headers(&wrong_name, &msg).unwrap_err();
        assert_eq!(e.status, 400);
        assert_eq!(e.body["error"]["code"], HEADER_MISMATCH);
        let no_method = HttpHeaders {
            method: None,
            ..ok.clone()
        };
        assert_eq!(
            check_http_headers(&no_method, &msg).unwrap_err().body["error"]["code"],
            HEADER_MISMATCH
        );
        // Header and _meta versions differ.
        let other = modern_call("2025-06-18", "list_models");
        assert_eq!(
            check_http_headers(&ok, &other).unwrap_err().body["error"]["code"],
            HEADER_MISMATCH
        );
        // Unknown version.
        let future = HttpHeaders {
            protocol_version: Some("2099-01-01".into()),
            ..ok.clone()
        };
        let e = check_http_headers(&future, &msg).unwrap_err();
        assert_eq!(e.body["error"]["code"], UNSUPPORTED_VERSION);
        assert_eq!(e.body["error"]["data"]["requested"], "2099-01-01");
        // Legacy clients: no header or a legacy version.
        assert!(check_http_headers(&HttpHeaders::default(), &msg).is_ok());
        let legacy = HttpHeaders {
            protocol_version: Some("2025-06-18".into()),
            ..Default::default()
        };
        assert!(check_http_headers(&legacy, &msg).is_ok());
    }
}
