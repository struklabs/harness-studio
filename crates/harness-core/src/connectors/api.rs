//! The provider API calls behind the connector tools. Every function takes the provider (so its
//! base URL can point at a mock server in tests) and an access token, validates its arguments
//! before any request, and returns compact JSON text for the model. Nothing here sends a
//! token anywhere but the provider's own API host.

use super::Provider;
use crate::tools::{ToolError, ToolErrorCode};
use base64::Engine;
use serde_json::{json, Value};

/// Largest response body read from a provider.
const MAX_BODY: usize = 2 * 1024 * 1024;
/// Most characters of a message or file body handed to the model.
const MAX_TEXT: usize = 12_000;

pub(super) struct Api<'a> {
    pub provider: &'a Provider,
    pub token: &'a str,
    client: reqwest::Client,
}

fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Provider object ids (Gmail, Drive, Graph) are short, URL-safe tokens.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.!~=".contains(&b))
}

fn id_arg(args: &Value, key: &str) -> Result<String, ToolError> {
    let id = args[key].as_str().unwrap_or_default().trim();
    if valid_id(id) {
        Ok(id.to_string())
    } else {
        Err(ToolError::invalid(format!(
            "{key} must be an id returned by a search tool"
        )))
    }
}

fn text_arg(args: &Value, key: &str, max: usize) -> Result<String, ToolError> {
    let v = args[key].as_str().unwrap_or_default();
    if v.trim().is_empty() {
        return Err(ToolError::invalid(format!("{key} must not be empty")));
    }
    if v.chars().count() > max {
        return Err(ToolError::invalid(format!(
            "{key} is longer than {max} characters"
        )));
    }
    Ok(v.to_string())
}

fn count_arg(args: &Value, key: &str, default: u64, max: u64) -> u64 {
    args.get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(1, max)
}

fn cut(text: &str) -> String {
    let n = text.chars().count();
    if n <= MAX_TEXT {
        return text.to_string();
    }
    let head: String = text.chars().take(MAX_TEXT).collect();
    format!("{head}\n…[{} more characters]", n - MAX_TEXT)
}

fn api_error(provider: &str, status: reqwest::StatusCode, body: &str) -> ToolError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("error_description"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default();
    let (code, what) = match status.as_u16() {
        401 => (
            ToolErrorCode::NotConfigured,
            "the sign-in was rejected; reconnect it in Settings > Tools > Connectors".to_string(),
        ),
        403 => (
            ToolErrorCode::Failed,
            "access was denied; the connector may lack the needed permission or the API is not enabled for the OAuth client".to_string(),
        ),
        404 => (ToolErrorCode::NotFound, "not found".to_string()),
        429 => (ToolErrorCode::Failed, "rate limit reached; retry later".to_string()),
        s => (ToolErrorCode::Failed, format!("HTTP {s}")),
    };
    let detail: String = detail.chars().take(300).collect();
    ToolError::new(
        code,
        if detail.is_empty() {
            format!("{provider}: {what}")
        } else {
            format!("{provider}: {what} ({detail})")
        },
    )
}

impl<'a> Api<'a> {
    pub fn new(provider: &'a Provider, token: &'a str) -> Result<Self, ToolError> {
        Ok(Self {
            provider,
            token,
            client: super::oauth::client().map_err(ToolError::failed)?,
        })
    }

    pub fn host(&self) -> String {
        reqwest::Url::parse(&self.provider.api_base)
            .ok()
            .and_then(|u| {
                u.host_str().map(|h| {
                    format!(
                        "{h}{}",
                        u.port().map(|p| format!(":{p}")).unwrap_or_default()
                    )
                })
            })
            .unwrap_or_default()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.provider.api_base.trim_end_matches('/'))
    }

    async fn send_raw(&self, req: reqwest::RequestBuilder) -> Result<String, ToolError> {
        let mut resp = req.bearer_auth(self.token).send().await.map_err(|_| {
            ToolError::new(
                ToolErrorCode::Network,
                format!("could not reach {}", self.provider.name),
            )
        })?;
        let status = resp.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|_| ToolError::new(ToolErrorCode::Network, "the response was interrupted"))?
        {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(ToolError::new(
                    ToolErrorCode::ResponseTooLarge,
                    "the response is larger than 2 MiB",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if status.is_success() {
            Ok(text)
        } else {
            Err(api_error(self.provider.name, status, &text))
        }
    }

    async fn json(&self, req: reqwest::RequestBuilder) -> Result<Value, ToolError> {
        let text = self.send_raw(req).await?;
        if text.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&text)
            .map_err(|_| ToolError::failed("the provider answered with something that is not JSON"))
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client.get(self.url(path))
    }
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn from_b64url(data: &str) -> Vec<u8> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data.trim_end_matches('='))
        .unwrap_or_default()
}

// --- Gmail ---

fn header(headers: &Value, name: &str) -> String {
    headers
        .as_array()
        .into_iter()
        .flatten()
        .find(|h| {
            h["name"]
                .as_str()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .and_then(|h| h["value"].as_str())
        .unwrap_or("")
        .chars()
        .take(300)
        .collect()
}

pub(super) async fn gmail_search(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let query = args["query"].as_str().unwrap_or_default();
    if query.chars().count() > 500 {
        return Err(ToolError::invalid("query is longer than 500 characters"));
    }
    let max = count_arg(args, "max_results", 10, 20);
    let list = api
        .json(
            api.get("/gmail/v1/users/me/messages")
                .query(&[("q", query.to_string()), ("maxResults", max.to_string())]),
        )
        .await?;
    let mut messages = vec![];
    for m in list["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .take(max as usize)
    {
        let Some(id) = m["id"].as_str().filter(|i| valid_id(i)) else {
            continue;
        };
        let v = api
            .json(
                api.get(&format!("/gmail/v1/users/me/messages/{id}"))
                    .query(&[
                        ("format", "metadata"),
                        ("metadataHeaders", "From"),
                        ("metadataHeaders", "Subject"),
                        ("metadataHeaders", "Date"),
                    ]),
            )
            .await?;
        let h = &v["payload"]["headers"];
        messages.push(json!({
            "id": id,
            "from": header(h, "From"),
            "subject": header(h, "Subject"),
            "date": header(h, "Date"),
            "snippet": v["snippet"],
        }));
    }
    Ok(json!({"messages": messages}).to_string())
}

fn collect_text(part: &Value, plain: &mut String, html: &mut String) {
    let mime = part["mimeType"].as_str().unwrap_or("");
    if let Some(data) = part["body"]["data"].as_str() {
        let text = String::from_utf8_lossy(&from_b64url(data)).into_owned();
        match mime {
            "text/plain" => plain.push_str(&text),
            "text/html" => html.push_str(&text),
            _ => {}
        }
    }
    for child in part["parts"].as_array().into_iter().flatten() {
        collect_text(child, plain, html);
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

pub(super) async fn gmail_read(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let id = id_arg(args, "id")?;
    let v = api
        .json(
            api.get(&format!("/gmail/v1/users/me/messages/{id}"))
                .query(&[("format", "full")]),
        )
        .await?;
    let (mut plain, mut html) = (String::new(), String::new());
    collect_text(&v["payload"], &mut plain, &mut html);
    let body = if plain.trim().is_empty() {
        strip_tags(&html)
    } else {
        plain
    };
    let h = &v["payload"]["headers"];
    Ok(json!({
        "id": id,
        "from": header(h, "From"),
        "to": header(h, "To"),
        "subject": header(h, "Subject"),
        "date": header(h, "Date"),
        "untrusted_content": cut(body.trim()),
    })
    .to_string())
}

fn address(a: &str) -> bool {
    let a = a.trim();
    let Some((local, domain)) = a.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.contains('@')
        && a.len() <= 254
        && !a
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "<>,;\"".contains(c))
}

/// An RFC 5322 message, base64url encoded for `users.messages.send`. Header values are checked
/// for line breaks first, so a model cannot add headers (Bcc, ...) through the subject.
pub(super) fn mime_message(to: &str, subject: &str, body: &str) -> Result<String, ToolError> {
    let recipients: Vec<&str> = to.split(',').map(str::trim).collect();
    if recipients.is_empty() || recipients.len() > 10 || !recipients.iter().all(|a| address(a)) {
        return Err(ToolError::invalid(
            "to must be one to ten plain email addresses separated by commas",
        ));
    }
    if subject.chars().any(char::is_control) || subject.chars().count() > 300 {
        return Err(ToolError::invalid(
            "subject must be a single line of up to 300 characters",
        ));
    }
    let subject = if subject.is_ascii() {
        subject.to_string()
    } else {
        format!(
            "=?UTF-8?B?{}?=",
            base64::engine::general_purpose::STANDARD.encode(subject)
        )
    };
    let encoded = base64::engine::general_purpose::STANDARD.encode(body);
    let wrapped: Vec<&str> = encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let message = format!(
        "To: {}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{}\r\n",
        recipients.join(", "),
        wrapped.join("\r\n")
    );
    Ok(b64url(message.as_bytes()))
}

pub(super) async fn gmail_send(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let to = text_arg(args, "to", 2000)?;
    let subject = text_arg(args, "subject", 300)?;
    let body = text_arg(args, "body", 50_000)?;
    let raw = mime_message(&to, &subject, &body)?;
    let v = api
        .json(
            api.client
                .post(api.url("/gmail/v1/users/me/messages/send"))
                .json(&json!({"raw": raw})),
        )
        .await?;
    Ok(json!({"sent": true, "id": v["id"], "to": to}).to_string())
}

// --- Google Drive ---

fn drive_quote(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

pub(super) async fn drive_search(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let text = text_arg(args, "query", 300)?;
    let q = format!(
        "(name contains '{0}' or fullText contains '{0}') and trashed = false",
        drive_quote(&text)
    );
    let max = count_arg(args, "max_results", 10, 25);
    let v = api
        .json(api.get("/drive/v3/files").query(&[
            ("q", q),
            ("pageSize", max.to_string()),
            (
                "fields",
                "files(id,name,mimeType,modifiedTime,size,webViewLink)".to_string(),
            ),
        ]))
        .await?;
    Ok(json!({"files": v["files"]}).to_string())
}

pub(super) async fn drive_read(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let id = id_arg(args, "id")?;
    let meta = api
        .json(
            api.get(&format!("/drive/v3/files/{id}"))
                .query(&[("fields", "id,name,mimeType,size")]),
        )
        .await?;
    let mime = meta["mimeType"].as_str().unwrap_or("");
    let content = if let Some(export) = match mime {
        "application/vnd.google-apps.document" | "application/vnd.google-apps.presentation" => {
            Some("text/plain")
        }
        "application/vnd.google-apps.spreadsheet" => Some("text/csv"),
        _ => None,
    } {
        api.send_raw(
            api.get(&format!("/drive/v3/files/{id}/export"))
                .query(&[("mimeType", export)]),
        )
        .await?
    } else if mime.starts_with("text/")
        || matches!(
            mime,
            "application/json" | "application/xml" | "application/x-yaml"
        )
    {
        if meta["size"]
            .as_str()
            .and_then(|s| s.parse::<usize>().ok())
            .is_some_and(|s| s > MAX_BODY)
        {
            return Err(ToolError::new(
                ToolErrorCode::ResponseTooLarge,
                "the file is larger than 2 MiB",
            ));
        }
        api.send_raw(
            api.get(&format!("/drive/v3/files/{id}"))
                .query(&[("alt", "media")]),
        )
        .await?
    } else {
        return Err(ToolError::new(
            ToolErrorCode::UnsupportedContentType,
            format!(
                "only text files and Google Docs, Sheets and Slides can be read (this is {mime})"
            ),
        ));
    };
    Ok(json!({"id": id, "name": meta["name"], "mimeType": mime, "untrusted_content": cut(&content)}).to_string())
}

pub(super) async fn drive_create_text_file(
    api: &Api<'_>,
    args: &Value,
) -> Result<String, ToolError> {
    let name = text_arg(args, "name", 200)?;
    if name.chars().any(char::is_control) {
        return Err(ToolError::invalid("name must be a single line"));
    }
    let content = text_arg(args, "content", 200_000)?;
    let created = api
        .json(
            api.client
                .post(api.url("/drive/v3/files"))
                .json(&json!({"name": name, "mimeType": "text/plain"})),
        )
        .await?;
    let id = created["id"]
        .as_str()
        .filter(|i| valid_id(i))
        .ok_or_else(|| ToolError::failed("Drive did not return a file id"))?
        .to_string();
    api.json(
        api.client
            .patch(api.url(&format!("/upload/drive/v3/files/{id}")))
            .query(&[("uploadType", "media")])
            .header("Content-Type", "text/plain; charset=UTF-8")
            .body(content),
    )
    .await?;
    Ok(json!({"created": true, "id": id, "name": name}).to_string())
}

// --- Google Calendar ---

fn rfc3339(args: &Value, key: &str) -> Result<Option<String>, ToolError> {
    match args[key].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => chrono::DateTime::parse_from_rfc3339(s)
            .map(|_| Some(s.to_string()))
            .map_err(|_| {
                ToolError::invalid(format!(
                    "{key} must be an RFC 3339 date-time such as 2026-10-21T09:30:00+03:00"
                ))
            }),
    }
}

pub(super) async fn calendar_list_events(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let time_min = rfc3339(args, "time_min")?
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    let mut query = vec![
        ("timeMin", time_min),
        (
            "maxResults",
            count_arg(args, "max_results", 10, 50).to_string(),
        ),
        ("singleEvents", "true".to_string()),
        ("orderBy", "startTime".to_string()),
    ];
    if let Some(max) = rfc3339(args, "time_max")? {
        query.push(("timeMax", max));
    }
    let v = api
        .json(
            api.get("/calendar/v3/calendars/primary/events")
                .query(&query),
        )
        .await?;
    let events: Vec<Value> = v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            json!({
                "id": e["id"], "summary": e["summary"], "start": e["start"], "end": e["end"],
                "location": e["location"], "status": e["status"],
            })
        })
        .collect();
    Ok(json!({"events": events}).to_string())
}

pub(super) async fn calendar_create_event(
    api: &Api<'_>,
    args: &Value,
) -> Result<String, ToolError> {
    let summary = text_arg(args, "summary", 300)?;
    let start = rfc3339(args, "start")?.ok_or_else(|| ToolError::invalid("start is required"))?;
    let end = rfc3339(args, "end")?.ok_or_else(|| ToolError::invalid("end is required"))?;
    let mut body = json!({
        "summary": summary,
        "start": {"dateTime": start},
        "end": {"dateTime": end},
    });
    for key in ["description", "location"] {
        if let Some(v) = args[key].as_str().filter(|v| !v.is_empty()) {
            body[key] = json!(v.chars().take(5000).collect::<String>());
        }
    }
    // No attendees and no invitations: the model cannot email third parties through this tool.
    let v = api
        .json(
            api.client
                .post(api.url("/calendar/v3/calendars/primary/events"))
                .query(&[("sendUpdates", "none")])
                .json(&body),
        )
        .await?;
    Ok(json!({"created": true, "id": v["id"], "link": v["htmlLink"]}).to_string())
}

// --- Excel (Microsoft Graph) ---

pub(super) async fn excel_find_workbooks(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let q = args["query"].as_str().unwrap_or_default().trim();
    if q.chars().count() > 200 {
        return Err(ToolError::invalid("query is longer than 200 characters"));
    }
    let q = if q.is_empty() { ".xlsx" } else { q };
    let path = format!(
        "/v1.0/me/drive/root/search(q='{}')?$select=id,name,webUrl,lastModifiedDateTime&$top=50",
        pct(&q.replace('\'', "''"))
    );
    let v = api.json(api.get(&path)).await?;
    let max = count_arg(args, "max_results", 10, 25) as usize;
    let files: Vec<&Value> = v["value"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|f| {
            f["name"]
                .as_str()
                .is_some_and(|n| n.to_ascii_lowercase().ends_with(".xlsx"))
        })
        .take(max)
        .collect();
    Ok(json!({"workbooks": files}).to_string())
}

fn range_path(args: &Value) -> Result<String, ToolError> {
    let id = id_arg(args, "workbook_id")?;
    let sheet = args["worksheet"].as_str().unwrap_or("Sheet1").trim();
    if sheet.is_empty()
        || sheet.chars().count() > 31
        || sheet
            .chars()
            .any(|c| c.is_control() || "\\/?*[]:'".contains(c))
    {
        return Err(ToolError::invalid(
            "worksheet must be a sheet name of up to 31 characters without \\ / ? * [ ] : '",
        ));
    }
    let address = args["address"].as_str().unwrap_or_default().trim();
    let cell = |s: &str| {
        let letters = s.bytes().take_while(u8::is_ascii_alphabetic).count();
        let digits = s.len() - letters;
        (1..=3).contains(&letters)
            && (1..=7).contains(&digits)
            && s.bytes().skip(letters).all(|b| b.is_ascii_digit())
    };
    let ok = match address.split_once(':') {
        Some((a, b)) => cell(a) && cell(b),
        None => cell(address),
    };
    if !ok {
        return Err(ToolError::invalid(
            "address must be a cell or range such as A1 or A1:C20",
        ));
    }
    Ok(format!(
        "/v1.0/me/drive/items/{id}/workbook/worksheets/{}/range(address='{address}')",
        pct(sheet)
    ))
}

pub(super) async fn excel_read_range(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let path = format!(
        "{}?$select=address,values,rowCount,columnCount",
        range_path(args)?
    );
    let v = api.json(api.get(&path)).await?;
    Ok(json!({
        "address": v["address"], "rows": v["rowCount"], "columns": v["columnCount"],
        "untrusted_values": v["values"],
    })
    .to_string())
}

pub(super) async fn excel_write_range(api: &Api<'_>, args: &Value) -> Result<String, ToolError> {
    let path = range_path(args)?;
    let rows = args["values"]
        .as_array()
        .filter(|r| !r.is_empty())
        .ok_or_else(|| ToolError::invalid("values must be a non-empty array of rows"))?;
    let mut cells = 0;
    for row in rows {
        let row = row
            .as_array()
            .ok_or_else(|| ToolError::invalid("every row of values must be an array"))?;
        cells += row.len();
        if row
            .iter()
            .any(|c| !(c.is_string() || c.is_number() || c.is_boolean() || c.is_null()))
        {
            return Err(ToolError::invalid(
                "cells must be text, numbers, booleans or null",
            ));
        }
    }
    if cells == 0 || cells > 2000 {
        return Err(ToolError::invalid("values must hold 1 to 2000 cells"));
    }
    let v = api
        .json(
            api.client
                .patch(api.url(&path))
                .json(&json!({"values": args["values"]})),
        )
        .await?;
    Ok(json!({"updated": true, "address": v["address"], "cells": cells}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::Family;
    use crate::tools::mcp_client::testserver::{serve, Req, Resp};
    use std::sync::{Arc, Mutex};

    fn provider(base: &str, name: &'static str) -> Provider {
        Provider {
            id: "mock",
            name,
            family: Family::Google,
            auth_url: String::new(),
            token_url: String::new(),
            revoke_url: None,
            api_base: base.into(),
            read_scopes: vec![],
            write_scopes: vec![],
            redirect_host: "127.0.0.1",
        }
    }

    fn b64(s: &str) -> String {
        b64url(s.as_bytes())
    }

    #[tokio::test]
    async fn gmail_search_read_and_send_against_a_mock_api() {
        let sent = Arc::new(Mutex::new(Vec::<Req>::new()));
        let sent2 = sent.clone();
        let server = serve(move |_, req| {
            assert_eq!(req.header("authorization"), Some("Bearer tok"));
            match (req.method.as_str(), req.path()) {
                ("GET", "/gmail/v1/users/me/messages") => {
                    assert_eq!(req.param("q").as_deref(), Some("from:ann"));
                    Resp::json(200, &json!({"messages":[{"id":"m1"}]}))
                }
                ("GET", "/gmail/v1/users/me/messages/m1") => {
                    if req.param("format").as_deref() == Some("full") {
                        Resp::json(200, &json!({"payload":{"headers":[{"name":"Subject","value":"Hi"}],"parts":[
                            {"mimeType":"text/plain","body":{"data":b64("Hello body")}},
                            {"mimeType":"text/html","body":{"data":b64("<b>ignored</b>")}}]}}))
                    } else {
                        Resp::json(200, &json!({"snippet":"snip","payload":{"headers":[
                            {"name":"From","value":"ann@example.com"},{"name":"Subject","value":"Hi"}]}}))
                    }
                }
                ("POST", "/gmail/v1/users/me/messages/send") => {
                    sent2.lock().unwrap().push(req.clone());
                    Resp::json(200, &json!({"id":"sent1"}))
                }
                _ => Resp::status(404),
            }
        })
        .await;
        let p = provider(&server.base, "Gmail");
        let api = Api::new(&p, "tok").unwrap();
        let found: Value = serde_json::from_str(
            &gmail_search(&api, &json!({"query":"from:ann"}))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(found["messages"][0]["from"], "ann@example.com");
        assert_eq!(found["messages"][0]["snippet"], "snip");
        let read: Value =
            serde_json::from_str(&gmail_read(&api, &json!({"id":"m1"})).await.unwrap()).unwrap();
        assert_eq!(read["untrusted_content"], "Hello body");
        assert!(gmail_read(&api, &json!({"id":"../x"})).await.is_err());

        let out = gmail_send(
            &api,
            &json!({"to":"bob@example.com","subject":"Tést","body":"Merhaba"}),
        )
        .await
        .unwrap();
        assert!(out.contains("sent1"));
        let body: Value = serde_json::from_str(&sent.lock().unwrap()[0].body).unwrap();
        let raw = String::from_utf8(from_b64url(body["raw"].as_str().unwrap())).unwrap();
        assert!(raw.starts_with("To: bob@example.com\r\nSubject: =?UTF-8?B?"));
        assert!(raw.contains("Content-Transfer-Encoding: base64"));
    }

    #[test]
    fn mail_headers_cannot_be_injected_and_recipients_are_checked() {
        assert!(mime_message("a@b.co", "Hi\r\nBcc: evil@x.co", "x").is_err());
        assert!(mime_message("a@b.co\r\nBcc: e@x.co", "Hi", "x").is_err());
        assert!(mime_message("not-an-address", "Hi", "x").is_err());
        assert!(mime_message("a@b.co, <c@d.co>", "Hi", "x").is_err());
        assert!(mime_message("a@b.co, c@d.co", "Hi", "x").is_ok());
        let eleven = (0..11)
            .map(|i| format!("u{i}@x.co"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(mime_message(&eleven, "Hi", "x").is_err());
    }

    #[tokio::test]
    async fn drive_search_read_and_create_against_a_mock_api() {
        let uploads = Arc::new(Mutex::new(Vec::<String>::new()));
        let up = uploads.clone();
        let server = serve(move |_, req| match (req.method.as_str(), req.path()) {
            ("GET", "/drive/v3/files") => {
                let q = req.param("q").unwrap();
                assert!(q.contains("name contains 'it\\'s'") && q.contains("trashed = false"));
                Resp::json(200, &json!({"files":[{"id":"f1","name":"Notes"}]}))
            }
            ("GET", "/drive/v3/files/doc1") => {
                Resp::json(200, &json!({"id":"doc1","name":"Doc","mimeType":"application/vnd.google-apps.document"}))
            }
            ("GET", "/drive/v3/files/doc1/export") => {
                assert_eq!(req.param("mimeType").as_deref(), Some("text/plain"));
                Resp::Full { status: 200, headers: vec![], body: "doc text".into() }
            }
            ("GET", "/drive/v3/files/img1") => {
                Resp::json(200, &json!({"id":"img1","name":"x.png","mimeType":"image/png"}))
            }
            ("POST", "/drive/v3/files") => {
                assert!(req.body.contains("\"name\":\"a.txt\""));
                Resp::json(200, &json!({"id":"new1"}))
            }
            ("PATCH", "/upload/drive/v3/files/new1") => {
                up.lock().unwrap().push(req.body.clone());
                Resp::json(200, &json!({"id":"new1"}))
            }
            _ => Resp::status(404),
        })
        .await;
        let p = provider(&server.base, "Google Drive");
        let api = Api::new(&p, "tok").unwrap();
        let files = drive_search(&api, &json!({"query":"it's"})).await.unwrap();
        assert!(files.contains("Notes"));
        let doc = drive_read(&api, &json!({"id":"doc1"})).await.unwrap();
        assert!(doc.contains("doc text") && doc.contains("untrusted_content"));
        let err = drive_read(&api, &json!({"id":"img1"})).await.unwrap_err();
        assert_eq!(err.code, ToolErrorCode::UnsupportedContentType);
        drive_create_text_file(&api, &json!({"name":"a.txt","content":"hello"}))
            .await
            .unwrap();
        assert_eq!(uploads.lock().unwrap().as_slice(), ["hello"]);
    }

    #[tokio::test]
    async fn calendar_list_and_create_against_a_mock_api() {
        let server = serve(|_, req| match (req.method.as_str(), req.path()) {
            ("GET", "/calendar/v3/calendars/primary/events") => {
                assert_eq!(req.param("singleEvents").as_deref(), Some("true"));
                assert!(req.param("timeMin").is_some());
                Resp::json(200, &json!({"items":[{"id":"e1","summary":"Standup","attendees":[{"email":"secret@x.co"}]}]}))
            }
            ("POST", "/calendar/v3/calendars/primary/events") => {
                assert_eq!(req.param("sendUpdates").as_deref(), Some("none"));
                assert!(!req.body.contains("attendees"));
                Resp::json(200, &json!({"id":"e2","htmlLink":"https://cal/e2"}))
            }
            _ => Resp::status(404),
        })
        .await;
        let p = provider(&server.base, "Google Calendar");
        let api = Api::new(&p, "tok").unwrap();
        let list = calendar_list_events(&api, &json!({})).await.unwrap();
        assert!(list.contains("Standup") && !list.contains("secret@x.co"));
        assert!(calendar_list_events(&api, &json!({"time_min":"tomorrow"}))
            .await
            .is_err());
        let made = calendar_create_event(
            &api,
            &json!({"summary":"Lunch","start":"2026-10-21T12:00:00+03:00","end":"2026-10-21T13:00:00+03:00","attendees":["x@y.co"]}),
        )
        .await
        .unwrap();
        assert!(made.contains("e2"));
        assert!(calendar_create_event(
            &api,
            &json!({"summary":"x","start":"2026-10-21T12:00:00Z"})
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn excel_search_read_and_write_against_a_mock_api() {
        let patches = Arc::new(Mutex::new(Vec::<String>::new()));
        let pa = patches.clone();
        let server = serve(move |_, req| {
            let target = req.target.clone();
            match req.method.as_str() {
                "GET" if target.starts_with("/v1.0/me/drive/root/search(q='it%27%27s')") => Resp::json(
                    200,
                    &json!({"value":[{"id":"w1","name":"Budget.xlsx"},{"id":"d1","name":"notes.docx"}]}),
                ),
                "GET" if target.starts_with("/v1.0/me/drive/items/w1/workbook/worksheets/Sheet%201/range(address='A1:B2')") => {
                    Resp::json(200, &json!({"address":"Sheet 1!A1:B2","rowCount":2,"columnCount":2,"values":[[1,2],[3,4]]}))
                }
                "PATCH" => {
                    pa.lock().unwrap().push(format!("{target} {}", req.body));
                    Resp::json(200, &json!({"address":"Sheet1!A1"}))
                }
                _ => Resp::status(404),
            }
        })
        .await;
        let p = provider(&server.base, "Excel");
        let api = Api::new(&p, "tok").unwrap();
        let found = excel_find_workbooks(&api, &json!({"query":"it's"}))
            .await
            .unwrap();
        assert!(found.contains("Budget.xlsx") && !found.contains("notes.docx"));
        let read = excel_read_range(
            &api,
            &json!({"workbook_id":"w1","worksheet":"Sheet 1","address":"A1:B2"}),
        )
        .await
        .unwrap();
        assert!(read.contains("untrusted_values"));
        excel_write_range(
            &api,
            &json!({"workbook_id":"w1","address":"A1","values":[["x"]]}),
        )
        .await
        .unwrap();
        assert!(patches.lock().unwrap()[0].contains("range(address='A1')"));
        for bad in [
            json!({"workbook_id":"w1","address":"A1'); drop"}),
            json!({"workbook_id":"../w1","address":"A1"}),
            json!({"workbook_id":"w1","worksheet":"a/b","address":"A1"}),
        ] {
            assert!(excel_read_range(&api, &bad).await.is_err(), "{bad}");
        }
        assert!(excel_write_range(
            &api,
            &json!({"workbook_id":"w1","address":"A1","values":[[{"a":1}]]})
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn provider_errors_are_explained_to_the_model() {
        let server = serve(|_, req| match req.path() {
            "/a" => Resp::json(401, &json!({"error":{"message":"Invalid Credentials"}})),
            "/b" => Resp::json(403, &json!({"error":{"message":"API disabled"}})),
            _ => Resp::status(500),
        })
        .await;
        let p = provider(&server.base, "Gmail");
        let api = Api::new(&p, "tok").unwrap();
        let e = api.json(api.get("/a")).await.unwrap_err();
        assert_eq!(e.code, ToolErrorCode::NotConfigured);
        assert!(e.message.contains("reconnect") && e.message.contains("Invalid Credentials"));
        let e = api.json(api.get("/b")).await.unwrap_err();
        assert!(e.message.contains("denied"));
        assert!(!e.message.contains("tok"));
    }
}
