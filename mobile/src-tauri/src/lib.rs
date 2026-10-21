//! Harness Studio mobile: the native side of the thin client. Two jobs, kept out of the WebView:
//!
//! * HTTP to a Harness gateway / Control API or a BYOK provider ([`http_request`], streamed with
//!   [`http_stream`]). Requests leave from Rust, so they carry no `Origin` and need no CORS
//!   change on the server.
//! * Secrets (Harness tokens, provider keys) in the platform keychain ([`secrets`]). The UI
//!   stores a key and later names it in a request; the key is added here and never read back
//!   into JavaScript. Each key is bound to the origin it was saved for and is sent nowhere else.

mod secrets;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, Manager, Runtime};

/// A whole answer (models lists, job lists, status): at most this many bytes.
const BODY_LIMIT: usize = 16 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scheme {
    /// `Authorization: Bearer <key>` (Harness tokens, OpenAI-style providers).
    Bearer,
    /// `x-api-key: <key>` (Anthropic).
    XApiKey,
}

/// A keychain entry the request is authenticated with.
#[derive(Debug, Deserialize)]
struct SecretAuth {
    account: String,
    scheme: Scheme,
}

#[derive(Debug, Deserialize)]
pub struct Request {
    method: String,
    url: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    secret: Option<SecretAuth>,
    /// The whole request (a stream: until the first byte).
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    Head {
        status: u16,
        headers: Vec<(String, String)>,
    },
    Data {
        text: String,
    },
    End,
    Error {
        message: String,
    },
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("HarnessStudioMobile/", env!("CARGO_PKG_VERSION")))
            // A redirect would carry the key to another origin.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("HTTP client")
    })
}

/// The origin (`scheme://host:port`) of an http(s) URL; anything else is refused.
fn origin_of(raw: &str) -> Result<(url::Url, String), String> {
    let url = url::Url::parse(raw).map_err(|_| "mobile.invalid_url".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("mobile.invalid_url".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("mobile.invalid_url".into());
    }
    let origin = url.origin().ascii_serialization();
    Ok((url, origin))
}

fn build<R: Runtime>(app: &AppHandle<R>, req: &Request) -> Result<reqwest::RequestBuilder, String> {
    let (url, origin) = origin_of(&req.url)?;
    let method = reqwest::Method::from_bytes(req.method.as_bytes())
        .map_err(|_| "mobile.invalid_request".to_string())?;
    let mut builder = client().request(method, url);
    for (name, value) in &req.headers {
        let lower = name.to_ascii_lowercase();
        // Credentials come from the keychain only.
        if matches!(
            lower.as_str(),
            "authorization" | "x-api-key" | "cookie" | "host"
        ) {
            return Err("mobile.invalid_request".into());
        }
        builder = builder.header(name, value);
    }
    if let Some(secret) = &req.secret {
        let key = secrets::key_for(app, &secret.account, &origin)?;
        builder = match secret.scheme {
            Scheme::Bearer => builder.bearer_auth(key),
            Scheme::XApiKey => builder.header("x-api-key", key),
        };
    }
    if let Some(body) = &req.body {
        builder = builder.body(body.clone());
    }
    Ok(builder)
}

fn transport(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "mobile.timeout".into()
    } else if error.is_connect() {
        "mobile.unreachable".into()
    } else {
        "mobile.transport".into()
    }
}

fn headers_of(response: &reqwest::Response) -> Vec<(String, String)> {
    response
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.as_str().to_string(), v.to_str().ok()?.to_string())))
        .collect()
}

#[tauri::command]
async fn http_request<R: Runtime>(app: AppHandle<R>, request: Request) -> Result<Response, String> {
    let timeout = request
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT);
    let response = build(&app, &request)?
        .timeout(timeout)
        .send()
        .await
        .map_err(transport)?;
    let status = response.status().as_u16();
    let headers = headers_of(&response);
    if response
        .content_length()
        .is_some_and(|n| n > BODY_LIMIT as u64)
    {
        return Err("mobile.too_large".into());
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(transport)?;
        if body.len() + chunk.len() > BODY_LIMIT {
            return Err("mobile.too_large".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Response {
        status,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn streams() -> &'static Mutex<HashMap<u32, tokio::task::AbortHandle>> {
    static STREAMS: OnceLock<Mutex<HashMap<u32, tokio::task::AbortHandle>>> = OnceLock::new();
    STREAMS.get_or_init(Default::default)
}

/// Splits `buffer` at the last complete UTF-8 character: (text to send, bytes kept).
fn take_utf8(buffer: &mut Vec<u8>) -> String {
    let valid = match std::str::from_utf8(buffer) {
        Ok(_) => buffer.len(),
        // An incomplete character at the end waits for the next chunk; an invalid one is
        // replaced.
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => buffer.len(),
    };
    let rest = buffer.split_off(valid);
    let text = String::from_utf8_lossy(buffer).into_owned();
    *buffer = rest;
    text
}

/// Starts a streamed request; its events go to `on_event` until `End` or `Error`. `id` names it
/// for [`http_cancel`].
#[tauri::command]
async fn http_stream<R: Runtime>(
    app: AppHandle<R>,
    id: u32,
    request: Request,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    let first_byte = request
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(300));
    let builder = build(&app, &request)?;
    let events = on_event.clone();
    let task = tauri::async_runtime::spawn(async move {
        let result: Result<(), String> = async {
            let response = tokio::time::timeout(first_byte, builder.send())
                .await
                .map_err(|_| "mobile.timeout".to_string())?
                .map_err(transport)?;
            let _ = events.send(StreamEvent::Head {
                status: response.status().as_u16(),
                headers: headers_of(&response),
            });
            let mut stream = response.bytes_stream();
            let mut pending = Vec::new();
            while let Some(chunk) = stream.next().await {
                pending.extend_from_slice(&chunk.map_err(transport)?);
                let text = take_utf8(&mut pending);
                if !text.is_empty() {
                    let _ = events.send(StreamEvent::Data { text });
                }
            }
            if !pending.is_empty() {
                let text = String::from_utf8_lossy(&pending).into_owned();
                let _ = events.send(StreamEvent::Data { text });
            }
            Ok(())
        }
        .await;
        let _ = events.send(match result {
            Ok(()) => StreamEvent::End,
            Err(message) => StreamEvent::Error { message },
        });
        streams().lock().unwrap().remove(&id);
    });
    streams()
        .lock()
        .unwrap()
        .insert(id, task.inner().abort_handle());
    Ok(())
}

/// Stops a streamed request (the user pressed Stop or left the chat).
#[tauri::command]
fn http_cancel(id: u32) {
    if let Some(handle) = streams().lock().unwrap().remove(&id) {
        handle.abort();
    }
}

/// Debug builds only: a scripted run for the emulator/simulator end-to-end test (see
/// `mobile/README.md`): the `HARNESS_MOBILE_E2E` environment (`xcrun simctl launch` passes
/// `SIMCTL_CHILD_HARNESS_MOBILE_E2E`), or `e2e.json` in the app's data folder (written with
/// `adb shell run-as` on Android), which is deleted once read. Release builds never read either.
#[tauri::command]
fn e2e_script<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    if let Ok(script) = std::env::var("HARNESS_MOBILE_E2E") {
        return Some(script);
    }
    let file = app.path().app_data_dir().ok()?.join("e2e.json");
    let script = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    Some(script)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(secrets::plugin())
        .invoke_handler(tauri::generate_handler![
            http_request,
            http_stream,
            http_cancel,
            e2e_script,
            secrets::secret_set,
            secrets::secret_has,
            secrets::secret_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Harness Studio mobile");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_http_urls_have_an_origin() {
        assert_eq!(
            origin_of("http://192.168.1.5:12440/v1/models").unwrap().1,
            "http://192.168.1.5:12440"
        );
        assert_eq!(
            origin_of("https://api.openai.com/v1/models").unwrap().1,
            "https://api.openai.com"
        );
        for bad in [
            "file:///etc/hosts",
            "ftp://x/",
            "http://user:pw@host/",
            "not a url",
            "data:text/plain,hi",
        ] {
            assert!(origin_of(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn credentials_never_come_from_the_page() {
        for name in ["Authorization", "x-api-key", "Cookie", "Host"] {
            let req = Request {
                method: "GET".into(),
                url: "https://example.com/".into(),
                headers: vec![(name.into(), "x".into())],
                body: None,
                secret: None,
                timeout_ms: None,
            };
            let app = tauri::test::mock_app();
            assert!(build(app.handle(), &req).is_err(), "{name}");
        }
    }

    #[test]
    fn stream_text_keeps_a_split_character_for_the_next_chunk() {
        let mut buffer = "data: ç".as_bytes().to_vec();
        let last = buffer.pop().unwrap();
        assert_eq!(take_utf8(&mut buffer), "data: ");
        assert_eq!(buffer.len(), 1);
        buffer.push(last);
        assert_eq!(take_utf8(&mut buffer), "ç");
        assert!(buffer.is_empty());
    }
}
