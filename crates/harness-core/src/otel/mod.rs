//! OpenTelemetry (OTLP) export of gateway traces, logs and metrics.
//!
//! Off by default and opt-in in Settings > OpenTelemetry; nothing is sent (and no connection
//! is opened) until the user turns it on. The export code sits behind the cargo feature
//! `otel` (the GUI and `harness` enable it); without it [`apply`] refuses to turn export on and
//! [`record`] does nothing.
//!
//! * **Traces:** one server span per finished gateway request (the `RequestRecord`), named
//!   `<operation> <model>` with the `gen_ai.*` / `http.*` attributes, child spans for the
//!   queue wait, the JIT load and generation (first token as an event). Timings come from the
//!   record, so the spans show exactly what Analytics shows.
//! * **Logs:** one `gateway_request` record with the same metadata Analytics stores, correlated
//!   with the server span.
//! * **Metrics:** request counter, operation duration, token usage, time to first token and
//!   queue wait histograms, exported every `metrics_interval_secs`.
//! * **Content:** prompts and answers are never exported, except when "include content" is on
//!   here *and* the model definition itself captures content (the same opt-in Analytics uses).
//! * **Headers** (an observability vendor's API key, say) are kept in the OS keychain; the
//!   settings only list their names.
//! * **Protocols:** OTLP/HTTP with protobuf bodies (`<endpoint>/v1/traces`, `/v1/logs`,
//!   `/v1/metrics`),
//!   sent with the workspace's reqwest 0.12. OTLP/gRPC only in builds with the cargo feature
//!   `otel-grpc` (tonic; not in the default builds, for size): elsewhere choosing it is
//!   [`ERR_GRPC_NOT_BUILT`] and [`OtelStatus::grpc`] is `false`.

use serde::{Deserialize, Serialize};

use crate::analytics::CapturedContent;
use crate::gateway::record::RequestRecord;

#[cfg(feature = "otel")]
mod export;

/// Error keys (i18n).
pub const ERR_ENDPOINT: &str = "otel.endpoint_invalid";
pub const ERR_HEADER: &str = "otel.header_invalid";
pub const ERR_SERVICE_NAME: &str = "otel.service_name_invalid";
pub const ERR_INTERVAL: &str = "otel.interval_invalid";
pub const ERR_NOT_BUILT: &str = "otel.not_built";
pub const ERR_GRPC_NOT_BUILT: &str = "otel.grpc_not_built";

pub const DEFAULT_HTTP_ENDPOINT: &str = "http://127.0.0.1:4318";
pub const DEFAULT_GRPC_ENDPOINT: &str = "http://127.0.0.1:4317";
pub const DEFAULT_SERVICE_NAME: &str = "harness-studio";
/// Longest prompt/answer text put on a span when content export is on.
pub const CONTENT_LIMIT: usize = 16 * 1024;

/// Wire protocol of the OTLP endpoint.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OtelProtocol {
    /// OTLP/HTTP with protobuf bodies (collector port 4318).
    #[default]
    HttpProtobuf,
    /// OTLP/gRPC (collector port 4317).
    Grpc,
}

/// `settings.json` → `otel`. Changed only through [`save_settings`] (the general settings
/// save keeps the stored block, like `gateway` and `control_api`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OtelSettings {
    /// Off by default.
    pub enabled: bool,
    /// Collector base URL (`http://host:4318` for HTTP, `http://host:4317` for gRPC). For HTTP
    /// the signal paths `/v1/traces`, `/v1/logs` and `/v1/metrics` are appended.
    pub endpoint: String,
    pub protocol: OtelProtocol,
    /// `service.name` resource attribute.
    pub service_name: String,
    pub traces: bool,
    /// Gateway request log records. Off by default, independently of traces.
    pub logs: bool,
    pub metrics: bool,
    /// Seconds between metric exports (10..=3600).
    pub metrics_interval_secs: u32,
    /// Put prompts and answers on spans, only for model definitions that capture content.
    pub include_content: bool,
    /// Names of the extra export headers; the values live in the OS keychain.
    pub header_names: Vec<String>,
}

impl Default for OtelSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: DEFAULT_HTTP_ENDPOINT.to_string(),
            protocol: OtelProtocol::HttpProtobuf,
            service_name: DEFAULT_SERVICE_NAME.to_string(),
            traces: true,
            logs: false,
            metrics: true,
            metrics_interval_secs: 60,
            include_content: false,
            header_names: Vec::new(),
        }
    }
}

impl OtelSettings {
    /// Full URL spans go to (for messages and the test result).
    pub fn traces_url(&self) -> String {
        match self.protocol {
            OtelProtocol::HttpProtobuf => format!("{}/v1/traces", self.endpoint),
            OtelProtocol::Grpc => self.endpoint.clone(),
        }
    }

    pub fn metrics_url(&self) -> String {
        match self.protocol {
            OtelProtocol::HttpProtobuf => format!("{}/v1/metrics", self.endpoint),
            OtelProtocol::Grpc => self.endpoint.clone(),
        }
    }

    pub fn logs_url(&self) -> String {
        match self.protocol {
            OtelProtocol::HttpProtobuf => format!("{}/v1/logs", self.endpoint),
            OtelProtocol::Grpc => self.endpoint.clone(),
        }
    }
}

/// Valid W3C Trace Context received with a gateway request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTraceContext {
    trace_id: [u8; 16],
    parent_span_id: [u8; 8],
    flags: u8,
    traceparent: String,
    tracestate: Option<String>,
}

impl RequestTraceContext {
    pub fn traceparent(&self) -> &str {
        &self.traceparent
    }

    pub fn tracestate(&self) -> Option<&str> {
        self.tracestate.as_deref()
    }
}

fn hex_bytes<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn valid_tracestate(raw: &str) -> bool {
    fn key_char(byte: u8) -> bool {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || matches!(byte, b'_' | b'-' | b'*' | b'/')
    }
    fn key(key: &str) -> bool {
        let parts: Vec<_> = key.split('@').collect();
        match parts.as_slice() {
            [simple] => {
                (1..=256).contains(&simple.len())
                    && simple.as_bytes()[0].is_ascii_lowercase()
                    && simple.bytes().all(key_char)
            }
            [tenant, system] => {
                (1..=241).contains(&tenant.len())
                    && (1..=14).contains(&system.len())
                    && (tenant.as_bytes()[0].is_ascii_lowercase()
                        || tenant.as_bytes()[0].is_ascii_digit())
                    && system.as_bytes()[0].is_ascii_lowercase()
                    && tenant.bytes().all(key_char)
                    && system.bytes().all(key_char)
            }
            _ => false,
        }
    }

    if raw.is_empty() || raw.len() > 512 {
        return false;
    }
    let mut keys = Vec::new();
    for member in raw.split(',') {
        let member = member.trim_matches([' ', '\t']);
        let Some((name, value)) = member.split_once('=') else {
            return false;
        };
        if !key(name)
            || !(1..=256).contains(&value.len())
            || value.ends_with(' ')
            || !value
                .bytes()
                .all(|b| (0x20..=0x7e).contains(&b) && b != b',' && b != b'=')
            || keys.contains(&name)
        {
            return false;
        }
        keys.push(name);
    }
    keys.len() <= 32
}

/// Parses W3C `traceparent` version 00. Invalid `tracestate` is discarded by the exporter,
/// while a valid `traceparent` still joins its trace as required by the specification.
pub fn parse_trace_context(
    traceparent: &str,
    tracestate: Option<&str>,
) -> Option<RequestTraceContext> {
    let mut fields = traceparent.split('-');
    let version = fields.next()?;
    let trace = fields.next()?;
    let parent = fields.next()?;
    let flags = fields.next()?;
    if fields.next().is_some() || version != "00" || traceparent.len() != 55 {
        return None;
    }
    let trace_id = hex_bytes(trace)?;
    let parent_span_id = hex_bytes(parent)?;
    let flags = hex_bytes::<1>(flags)?[0];
    if trace_id.iter().all(|b| *b == 0) || parent_span_id.iter().all(|b| *b == 0) {
        return None;
    }
    Some(RequestTraceContext {
        trace_id,
        parent_span_id,
        flags,
        traceparent: traceparent.to_string(),
        tracestate: tracestate
            .map(str::trim)
            .filter(|s| valid_tracestate(s))
            .map(str::to_string),
    })
}

/// Canonical collector URL: `http(s)://host[:port][/path]`, no trailing slash, no signal path
/// (a pasted `.../v1/traces` is reduced to its base).
pub fn normalize_endpoint(raw: &str) -> Result<String, String> {
    let mut e = raw.trim().trim_end_matches('/').to_string();
    for suffix in ["/v1/traces", "/v1/metrics", "/v1/logs"] {
        if let Some(base) = e.strip_suffix(suffix) {
            e = base.trim_end_matches('/').to_string();
        }
    }
    let lower = e.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("https://"))
        .ok_or_else(|| ERR_ENDPOINT.to_string())?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty()
        || host.starts_with(':')
        || e.chars().any(|c| c.is_whitespace() || c.is_control())
        || rest.contains(['?', '#', '@'])
    {
        return Err(ERR_ENDPOINT.to_string());
    }
    Ok(e)
}

fn is_tchar(c: char) -> bool {
    c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c)
}

/// Parses export headers: one `name=value` or `name: value` per line, or comma-separated
/// `name=value` pairs (the `OTEL_EXPORTER_OTLP_HEADERS` format). Names are HTTP tokens
/// (stored lowercase); values are visible ASCII without line breaks.
pub fn parse_headers(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out: Vec<(String, String)> = Vec::new();
    let single_line = !text.trim().contains('\n');
    let parts: Vec<&str> = if single_line && !text.contains(':') {
        text.split(',').collect()
    } else {
        text.lines().collect()
    };
    for part in parts.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
        let (name, value) = match (part.find('='), part.find(':')) {
            (Some(eq), Some(colon)) => part.split_at(eq.min(colon)),
            (Some(i), None) | (None, Some(i)) => part.split_at(i),
            (None, None) => return Err(ERR_HEADER.to_string()),
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value[1..].trim().to_string();
        if name.is_empty()
            || !name.chars().all(is_tchar)
            || value.is_empty()
            || !value
                .chars()
                .all(|c| c == ' ' || c == '\t' || c.is_ascii_graphic())
        {
            return Err(ERR_HEADER.to_string());
        }
        out.retain(|(n, _)| *n != name);
        out.push((name, value));
    }
    Ok(out)
}

fn headers_text(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .map(|(n, v)| format!("{}={}", n, v))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Validates settings coming from a front end (the stored header names are kept; they change
/// only with the header values in [`save_settings`]). gRPC in a build without it is
/// [`ERR_GRPC_NOT_BUILT`].
pub fn validate_settings(incoming: &OtelSettings) -> Result<OtelSettings, String> {
    let mut s = incoming.clone();
    if s.protocol == OtelProtocol::Grpc && !grpc_built() {
        return Err(crate::error_key::err(ERR_GRPC_NOT_BUILT).into());
    }
    if s.endpoint.trim().is_empty() {
        s.endpoint = match s.protocol {
            OtelProtocol::HttpProtobuf => DEFAULT_HTTP_ENDPOINT,
            OtelProtocol::Grpc => DEFAULT_GRPC_ENDPOINT,
        }
        .to_string();
    }
    s.endpoint = normalize_endpoint(&s.endpoint)?;
    s.service_name = s.service_name.trim().to_string();
    if s.service_name.is_empty() {
        s.service_name = DEFAULT_SERVICE_NAME.to_string();
    }
    if s.service_name.chars().count() > 128 || s.service_name.chars().any(|c| c.is_control()) {
        return Err(ERR_SERVICE_NAME.to_string());
    }
    if !(10..=3600).contains(&s.metrics_interval_secs) {
        return Err(ERR_INTERVAL.to_string());
    }
    Ok(s)
}

/// The export headers saved in the keychain.
pub fn saved_headers() -> Result<Vec<(String, String)>, String> {
    match crate::secrets::otel_headers_get()? {
        Some(text) => parse_headers(&text),
        None => Ok(Vec::new()),
    }
}

/// Validates and saves the settings. `headers`: `None` keeps the saved headers, `Some("")`
/// removes them, anything else replaces them (keychain). Returns the stored settings; the
/// caller applies them with [`apply`].
pub fn save_settings(
    incoming: &OtelSettings,
    headers: Option<&str>,
) -> Result<OtelSettings, String> {
    let mut s = validate_settings(incoming)?;
    let mut all = crate::config::AppSettings::load();
    s.header_names = all.otel.header_names.clone();
    if let Some(text) = headers {
        let parsed = parse_headers(text)?;
        if parsed.is_empty() {
            crate::secrets::otel_headers_delete()?;
        } else {
            crate::secrets::otel_headers_set(&headers_text(&parsed))?;
        }
        s.header_names = parsed.into_iter().map(|(n, _)| n).collect();
    }
    all.otel = s.clone();
    all.save()?;
    Ok(s)
}

/// Exporter state for Settings and diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct OtelStatus {
    /// This build contains the exporter (cargo feature `otel`).
    pub built: bool,
    /// This build can export over OTLP/gRPC (cargo feature `otel-grpc`); OTLP/HTTP is always
    /// there when `built`.
    #[serde(default)]
    pub grpc: bool,
    /// Export is running.
    pub active: bool,
    pub traces_url: Option<String>,
    pub metrics_url: Option<String>,
    pub logs_url: Option<String>,
    pub spans_exported: u64,
    /// Spans dropped because the export queue was full.
    pub spans_dropped: u64,
    pub logs_exported: u64,
    pub logs_dropped: u64,
    pub export_errors: u64,
    pub last_error: Option<String>,
    /// Unix ms of the last successful span export.
    pub last_export_ms: Option<u64>,
}

/// Result of "Test export": one real span sent to the configured endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TestExportResult {
    pub ok: bool,
    /// Where the span went.
    pub url: String,
    pub protocol: OtelProtocol,
    pub elapsed_ms: u64,
    /// Trace id of the test span (search for it in the backend).
    pub trace_id: String,
    pub error: Option<String>,
}

/// Whether this build can export (cargo feature `otel`).
pub const fn built() -> bool {
    cfg!(feature = "otel")
}

/// Whether this build can export over OTLP/gRPC (cargo feature `otel-grpc`).
pub const fn grpc_built() -> bool {
    cfg!(feature = "otel-grpc")
}

/// Starts, restarts or stops the exporter to match `settings` (call from a Tokio runtime).
/// Turning it on without the `otel` feature is [`ERR_NOT_BUILT`].
pub async fn apply(settings: &OtelSettings) -> Result<(), String> {
    #[cfg(feature = "otel")]
    {
        export::apply(settings).await
    }
    #[cfg(not(feature = "otel"))]
    {
        if settings.enabled {
            Err(ERR_NOT_BUILT.to_string())
        } else {
            Ok(())
        }
    }
}

/// Exports whatever is queued (spans and metrics) and waits for it, at most `timeout`.
pub async fn flush(timeout: std::time::Duration) -> bool {
    #[cfg(feature = "otel")]
    {
        export::flush(timeout).await
    }
    #[cfg(not(feature = "otel"))]
    {
        let _ = timeout;
        true
    }
}

/// Stops the exporter after a final flush (app exit).
pub async fn shutdown() {
    #[cfg(feature = "otel")]
    export::shutdown().await;
}

/// Hands one finished gateway request to the exporter (non-blocking; a no-op when export is
/// off). `content` is `Some` only for definitions with content capture on.
pub fn record(
    record: &RequestRecord,
    content: Option<&CapturedContent>,
    trace_context: Option<&RequestTraceContext>,
) {
    #[cfg(feature = "otel")]
    export::record(record, content, trace_context);
    #[cfg(not(feature = "otel"))]
    let _ = (record, content, trace_context);
}

pub fn status() -> OtelStatus {
    #[cfg(feature = "otel")]
    {
        export::status()
    }
    #[cfg(not(feature = "otel"))]
    {
        OtelStatus::default()
    }
}

/// Sends one test span with `settings` (saved or not). `headers`: `None` = the saved
/// headers, `Some(text)` = these (not yet saved) headers.
pub async fn test_export(
    settings: &OtelSettings,
    headers: Option<&str>,
) -> Result<TestExportResult, String> {
    let settings = validate_settings(settings)?;
    let headers = match headers {
        Some(text) => parse_headers(text)?,
        None => saved_headers()?,
    };
    #[cfg(feature = "otel")]
    {
        Ok(export::test_export(&settings, &headers).await)
    }
    #[cfg(not(feature = "otel"))]
    {
        let _ = (settings, headers);
        Err(ERR_NOT_BUILT.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_canonical() {
        assert_eq!(
            normalize_endpoint(" http://localhost:4318/ ").unwrap(),
            "http://localhost:4318"
        );
        assert_eq!(
            normalize_endpoint("https://otlp.example.com/otlp/v1/traces").unwrap(),
            "https://otlp.example.com/otlp"
        );
        assert_eq!(
            normalize_endpoint("http://[::1]:4317").unwrap(),
            "http://[::1]:4317"
        );
        for bad in [
            "",
            "localhost:4318",
            "ftp://x",
            "http://",
            "http://:4318",
            "http://a b",
            "http://u:p@host",
            "http://h/?q=1",
        ] {
            assert_eq!(
                normalize_endpoint(bad).unwrap_err(),
                ERR_ENDPOINT,
                "{bad:?}"
            );
        }
    }

    #[test]
    fn headers_parse_in_every_usual_format() {
        let expect = vec![
            ("authorization".to_string(), "Bearer abc=".to_string()),
            ("x-team".to_string(), "t1".to_string()),
        ];
        assert_eq!(
            parse_headers("Authorization: Bearer abc=\nx-team: t1").unwrap(),
            expect
        );
        assert_eq!(
            parse_headers("authorization=Bearer abc=\n\n x-team = t1 ").unwrap(),
            expect
        );
        assert_eq!(
            parse_headers("x-team=t1,api-key=k").unwrap(),
            vec![
                ("x-team".to_string(), "t1".to_string()),
                ("api-key".to_string(), "k".to_string())
            ]
        );
        // Later duplicates win.
        assert_eq!(
            parse_headers("a=1\nA=2").unwrap(),
            vec![("a".to_string(), "2".to_string())]
        );
        assert!(parse_headers("").unwrap().is_empty());
        for bad in ["novalue", "bad name=1", "x=", "x=a\u{7f}", "x=ü"] {
            assert_eq!(parse_headers(bad).unwrap_err(), ERR_HEADER, "{bad:?}");
        }
    }

    #[test]
    fn settings_validation() {
        let d = validate_settings(&OtelSettings::default()).unwrap();
        assert!(!d.enabled, "off by default");
        assert_eq!(d.traces_url(), "http://127.0.0.1:4318/v1/traces");
        assert_eq!(d.logs_url(), "http://127.0.0.1:4318/v1/logs");
        assert!(!d.logs, "logs are separately opt-in");
        assert_eq!(d.metrics_url(), "http://127.0.0.1:4318/v1/metrics");
        let grpc = validate_settings(&OtelSettings {
            protocol: OtelProtocol::Grpc,
            endpoint: " ".into(),
            service_name: "  ".into(),
            ..Default::default()
        });
        if grpc_built() {
            let g = grpc.unwrap();
            assert_eq!(g.endpoint, DEFAULT_GRPC_ENDPOINT);
            assert_eq!(g.traces_url(), DEFAULT_GRPC_ENDPOINT);
            assert_eq!(g.service_name, DEFAULT_SERVICE_NAME);
        } else {
            // A clear, keyed error instead of a protocol the build cannot speak.
            assert_eq!(grpc.unwrap_err(), ERR_GRPC_NOT_BUILT);
        }
        assert_eq!(status().grpc, grpc_built());
        assert_eq!(
            serde_json::to_value(OtelProtocol::Grpc).unwrap(),
            "grpc",
            "stored settings keep their protocol value either way"
        );
        assert_eq!(
            validate_settings(&OtelSettings {
                metrics_interval_secs: 5,
                ..Default::default()
            })
            .unwrap_err(),
            ERR_INTERVAL
        );
        assert_eq!(
            serde_json::to_value(OtelProtocol::HttpProtobuf).unwrap(),
            "http_protobuf"
        );
        // Old settings files have no `otel` block.
        let parsed: OtelSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, OtelSettings::default());
    }

    #[test]
    fn traceparent_version_00_parses() {
        let c = parse_trace_context(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            Some("vendor=value"),
        )
        .unwrap();
        assert_eq!(
            c.trace_id,
            hex_bytes("4bf92f3577b34da6a3ce929d0e0e4736").unwrap()
        );
        assert_eq!(c.parent_span_id, hex_bytes("00f067aa0ba902b7").unwrap());
        assert_eq!(c.flags, 1);
        assert_eq!(c.tracestate(), Some("vendor=value"));
        assert_eq!(
            parse_trace_context(c.traceparent(), Some("Vendor=value"))
                .unwrap()
                .tracestate(),
            None
        );
    }

    #[test]
    fn traceparent_rejects_invalid_values() {
        for bad in [
            "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
            "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e473g-00f067aa0ba902b7-01",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        ] {
            assert!(parse_trace_context(bad, None).is_none(), "{bad}");
        }
    }

    #[test]
    fn traceparent_rejects_all_zero_ids() {
        assert!(parse_trace_context(
            "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
            None
        )
        .is_none());
        assert!(parse_trace_context(
            "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
            None
        )
        .is_none());
    }
}
