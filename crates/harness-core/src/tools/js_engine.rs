//! The JS runtime of installed tools: QuickJS through `rquickjs`.
//!
//! QuickJS itself has no file system, network, process or module loader: a fresh context sees
//! only the language. Pure JS polyfills add missing base64 and UTF-8 conversion globals.
//! The only host capability is the global `harness`, whose functions call back into Rust,
//! and the manifest's permissions are applied *there*, before anything touches the
//! outside world:
//!
//! * `harness.http.get(url)` / `harness.http.post(url, jsonBody)` -> `{status, content_type,
//!   url, body}`. The host must be on the manifest's `network` list on the first request and on
//!   every redirect; the request then goes through the same public-address check, resolver pin
//!   and size limit as the built-in `fetch_url` (so a private or link-local address is refused
//!   even when a hostname on the list resolves to it).
//! * `harness.fs.read(path)` / `.write(path, text)` / `.list(path)`: paths are relative to the
//!   chat workspace and must stay inside a folder the manifest grants for that direction, after
//!   the same symlink-resolving check the file tools use ([`Workspace`]).
//!
//! The entry file defines `function run(args)`; it may return a string or any JSON value, or a
//! promise of one (the host functions are synchronous, so `async` is only syntax). A call is
//! serialized as plain string text or JSON; `null` and `undefined` produce the text `"null"`
//! in normal execution as well as tests (an empty string stays empty). Each call is
//! bounded by a heap limit, a stack limit, a deadline and the call's cancellation token, both
//! enforced through QuickJS's interrupt hook, and a host call waiting on the network also
//! listens for the deadline and the cancellation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use rquickjs::{Context, Ctx, Function, Runtime, Value};
use serde_json::{json, Value as Json};
use tokio::runtime::Handle;

use super::builtin::http;
use super::package::{domain_allowed, Permissions};
use super::registry::ToolCtx;
use super::scope::Workspace;
use super::types::{ToolError, ToolErrorCode, ToolOutput};

/// Heap one call may use.
pub const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
const STACK_LIMIT: usize = 1024 * 1024;
/// Largest HTTP response body and file the host hands to the code.
const MAX_BODY: usize = 2 * 1024 * 1024;
const MAX_FILE: u64 = 1024 * 1024;
const MAX_LISTING: usize = 1000;

/// Runs once per call; returns the function that calls `run(args)`. `host` is reachable only
/// through the closure below, never as a global.
const PRELUDE: &str = r#"
(function (host) {
  // Pure data conversions only: no new host capabilities. Keep native implementations.
  const install = (name, value) => {
    if (typeof globalThis[name] === 'undefined') {
      Object.defineProperty(globalThis, name, { value, writable: true, configurable: true });
    }
  };
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const invalidBase64 = () => {
    const e = new Error('Invalid base64 or Latin-1 input');
    e.name = 'InvalidCharacterError';
    throw e;
  };
  install('btoa', function (input) {
    const text = String(input);
    let result = '';
    for (let i = 0; i < text.length; i += 3) {
      const a = text.charCodeAt(i), b = text.charCodeAt(i + 1), c = text.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) invalidBase64();
      const bits = (a << 16) | ((b || 0) << 8) | (c || 0);
      result += alphabet[(bits >>> 18) & 63] + alphabet[(bits >>> 12) & 63]
        + (i + 1 < text.length ? alphabet[(bits >>> 6) & 63] : '=')
        + (i + 2 < text.length ? alphabet[bits & 63] : '=');
    }
    return result;
  });
  install('atob', function (input) {
    // Web-compatible forgiving base64: ASCII whitespace and omitted padding are allowed.
    let text = String(input).replace(/[\t\n\f\r ]/g, '');
    if (text.length % 4 === 0) text = text.replace(/==?$/, '');
    if (text.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(text)) invalidBase64();
    let result = '', bits = 0, count = 0;
    for (const ch of text) {
      bits = (bits << 6) | alphabet.indexOf(ch);
      count += 6;
      if (count >= 8) {
        count -= 8;
        result += String.fromCharCode((bits >>> count) & 255);
      }
    }
    return result;
  });
  install('TextEncoder', class TextEncoder {
    get encoding() { return 'utf-8'; }
    encode(input = '') {
      const bytes = [];
      for (const ch of String(input)) {
        let cp = ch.codePointAt(0);
        if (cp >= 0xd800 && cp <= 0xdfff) cp = 0xfffd;
        if (cp < 0x80) bytes.push(cp);
        else if (cp < 0x800) bytes.push(0xc0 | (cp >> 6), 0x80 | (cp & 63));
        else if (cp < 0x10000) bytes.push(0xe0 | (cp >> 12), 0x80 | ((cp >> 6) & 63), 0x80 | (cp & 63));
        else bytes.push(0xf0 | (cp >> 18), 0x80 | ((cp >> 12) & 63), 0x80 | ((cp >> 6) & 63), 0x80 | (cp & 63));
      }
      return new Uint8Array(bytes);
    }
  });
  install('TextDecoder', class TextDecoder {
    constructor(label = 'utf-8', options = {}) {
      if (!['utf-8', 'utf8', 'unicode-1-1-utf-8'].includes(String(label).trim().toLowerCase())) {
        throw new RangeError('Only UTF-8 decoding is supported');
      }
      this.encoding = 'utf-8';
      this.fatal = Boolean(options.fatal);
      this.ignoreBOM = Boolean(options.ignoreBOM);
    }
    decode(input = new Uint8Array(), options = {}) {
      if (options.stream) throw new TypeError('Streaming decoding is not supported');
      let bytes;
      if (input instanceof ArrayBuffer) bytes = new Uint8Array(input);
      else if (ArrayBuffer.isView(input)) bytes = new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
      else throw new TypeError('Expected an ArrayBuffer or typed array');
      let result = '';
      const bad = () => {
        if (this.fatal) throw new TypeError('Invalid UTF-8 input');
        result += '\ufffd';
      };
      for (let i = 0; i < bytes.length;) {
        const first = bytes[i++];
        if (first < 0x80) { result += String.fromCharCode(first); continue; }
        const size = first >= 0xc2 && first <= 0xdf ? 2
          : first >= 0xe0 && first <= 0xef ? 3 : first >= 0xf0 && first <= 0xf4 ? 4 : 0;
        if (!size) { bad(); continue; }
        let cp = first & (0x7f >> size), valid = true;
        for (let n = 1; n < size; n++) {
          const next = bytes[i];
          const lower = n === 1 && first === 0xe0 ? 0xa0 : n === 1 && first === 0xf0 ? 0x90 : 0x80;
          const upper = n === 1 && first === 0xed ? 0x9f : n === 1 && first === 0xf4 ? 0x8f : 0xbf;
          if (next === undefined || next < lower || next > upper) { valid = false; break; }
          cp = (cp << 6) | (next & 63);
          i++;
        }
        if (valid) result += String.fromCodePoint(cp);
        else bad();
      }
      return !this.ignoreBOM && result.charCodeAt(0) === 0xfeff ? result.slice(1) : result;
    }
  });
  const call = (op, a, b) => {
    const r = JSON.parse(host(op, a === undefined ? '' : String(a), b === undefined ? '' : b));
    if (r.err) { const e = new Error(r.err.message); e.code = r.err.code; throw e; }
    return r.ok;
  };
  const harness = Object.freeze({
    http: Object.freeze({
      get: (url) => call('http.get', url),
      post: (url, body) => call('http.post', url, JSON.stringify(body === undefined ? null : body)),
    }),
    fs: Object.freeze({
      read: (path) => call('fs.read', path),
      write: (path, text) => call('fs.write', path, String(text)),
      list: (path) => call('fs.list', path),
    }),
  });
  Object.defineProperty(globalThis, 'harness', { value: harness });
  return (json) => {
    if (typeof run !== 'function') throw new Error('the entry file must define function run(args)');
    const fin = (r) => r === undefined ? 'null' : typeof r === 'string' ? r : (JSON.stringify(r) ?? '');
    const r = run(JSON.parse(json));
    return (r && typeof r.then === 'function') ? r.then(fin) : fin(r);
  };
})
"#;

fn code_from(name: &str) -> ToolErrorCode {
    match name {
        "outside_workspace" => ToolErrorCode::OutsideWorkspace,
        "no_workspace" => ToolErrorCode::NoWorkspace,
        "not_found" => ToolErrorCode::NotFound,
        "io" => ToolErrorCode::Io,
        "network" => ToolErrorCode::Network,
        "response_too_large" => ToolErrorCode::ResponseTooLarge,
        "unsupported_content_type" => ToolErrorCode::UnsupportedContentType,
        "timeout" => ToolErrorCode::Timeout,
        "cancelled" => ToolErrorCode::Cancelled,
        "invalid_arguments" => ToolErrorCode::InvalidArguments,
        _ => ToolErrorCode::Failed,
    }
}

fn denied(code: ToolErrorCode, message: impl Into<String>) -> ToolError {
    ToolError::new(code, message)
}

/// What the host functions of one call may do.
struct Host {
    perms: Permissions,
    ctx: ToolCtx,
    handle: Handle,
    deadline: Instant,
    active_mocks: std::sync::Mutex<Option<Vec<Json>>>,
    test_workspace: std::sync::Mutex<Option<Workspace>>,
    offline: bool,
}

impl Host {
    fn dispatch(&self, op: &str, a: &str, b: &str) -> Result<Json, ToolError> {
        match op {
            "http.get" => self.http("GET", a, None),
            "http.post" => {
                let body: Json = serde_json::from_str(b).map_err(|e| {
                    ToolError::invalid(format!("the request body is not JSON: {e}"))
                })?;
                self.http("POST", a, Some(body))
            }
            "fs.read" => self.fs_read(a),
            "fs.write" => self.fs_write(a, b),
            "fs.list" => self.fs_list(a),
            _ => Err(ToolError::failed(format!("unknown host function {op}"))),
        }
    }

    /// Waits for `fut` unless the call is cancelled or its time is up.
    fn block_on<T>(
        &self,
        fut: impl std::future::Future<Output = Result<T, ToolError>>,
    ) -> Result<T, ToolError> {
        let deadline = tokio::time::Instant::from_std(self.deadline);
        self.handle.block_on(async {
            tokio::select! {
                r = fut => r,
                _ = tokio::time::sleep_until(deadline) => Err(denied(
                    ToolErrorCode::Timeout,
                    "the tool did not finish in time and was stopped",
                )),
                _ = self.ctx.cancel.cancelled() => Err(denied(
                    ToolErrorCode::Cancelled,
                    "the call was cancelled",
                )),
            }
        })
    }

    fn http(&self, method: &str, raw: &str, body: Option<Json>) -> Result<Json, ToolError> {
        let allowed = self.perms.network.clone();
        let check = move |url: &reqwest::Url| -> Result<(), ToolError> {
            let host = url.host_str().unwrap_or_default();
            if domain_allowed(&allowed, host) {
                Ok(())
            } else {
                Err(denied(
                    ToolErrorCode::Network,
                    format!(
                        "this tool may not contact {host}: it is not in the tool's allowed hosts"
                    ),
                )
                .network_target(http::target(url)))
            }
        };
        let method = if method == "POST" {
            reqwest::Method::POST
        } else {
            reqwest::Method::GET
        };
        let trusted = reqwest::Url::parse(raw)
            .ok()
            .map(|u| http::target(&u))
            .filter(|t| trusted_test_host(t));

        let mocks_opt = self.active_mocks.lock().unwrap().clone();
        if let Some(mocks) = mocks_opt {
            let url = reqwest::Url::parse(raw)
                .map_err(|e| ToolError::invalid(format!("invalid url: {e}")))?;
            check(&url)?;
            for m in &mocks {
                let m_method = m.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
                if !m_method.eq_ignore_ascii_case(method.as_str()) {
                    continue;
                }
                let matches = if let Some(exact) = m.get("url").and_then(|v| v.as_str()) {
                    raw == exact
                } else if let Some(prefix) = m.get("url_prefix").and_then(|v| v.as_str()) {
                    raw.starts_with(prefix)
                } else {
                    false
                };
                if matches {
                    let status = m.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
                    let mut content_type = String::new();
                    if let Some(h) = m.get("headers").and_then(|v| v.as_object()) {
                        for (k, v) in h {
                            if k.eq_ignore_ascii_case("content-type") {
                                content_type = v
                                    .as_str()
                                    .unwrap_or("")
                                    .split(';')
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_ascii_lowercase();
                            }
                        }
                    }
                    let empty_json = json!("");
                    let body_val = m.get("body").unwrap_or(&empty_json);
                    let body_str = if let Some(s) = body_val.as_str() {
                        s.to_string()
                    } else {
                        serde_json::to_string(body_val).unwrap_or_default()
                    };
                    if body_str.len() > MAX_BODY {
                        return Err(ToolError::new(
                            ToolErrorCode::ResponseTooLarge,
                            "mock body too large",
                        ));
                    }
                    if content_type.is_empty() && !body_val.is_string() {
                        content_type = "application/json".to_string();
                    }
                    return Ok(json!({
                        "status": status,
                        "content_type": content_type,
                        "url": raw,
                        "body": body_str,
                    }));
                }
            }
            return Err(ToolError::failed(format!("no mock for {raw}")));
        }

        if self.offline {
            let url = reqwest::Url::parse(raw)
                .map_err(|e| ToolError::invalid(format!("invalid url: {e}")))?;
            let host = url.host_str().unwrap_or_default();
            return Err(denied(
                ToolErrorCode::Network,
                format!("this tool may not contact {host}: it is not in the tool's allowed hosts"),
            )
            .network_target(http::target(&url)));
        }

        self.block_on(async {
            let (response, url) =
                http::request_checked(method, raw, &self.ctx, body, trusted, &check).await?;
            let target = http::target(&url);
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let texty = content_type.is_empty()
                || content_type.starts_with("text/")
                || ["json", "xml", "javascript"]
                    .iter()
                    .any(|k| content_type.contains(k));
            if !texty {
                return Err(ToolError::new(
                    ToolErrorCode::UnsupportedContentType,
                    format!("refused content type {content_type:?}: only text, HTML, JSON and XML are returned"),
                )
                .network_target(target));
            }
            let bytes = http::bytes_limited(response, MAX_BODY, &target).await?;
            Ok(json!({
                "status": status,
                "content_type": content_type,
                "url": url.as_str(),
                "body": String::from_utf8_lossy(&bytes),
            }))
        })
    }

    /// The path resolved inside the workspace and inside one of the granted `scopes`.
    fn scoped(
        &self,
        scopes: &[String],
        raw: &str,
        must_exist: bool,
    ) -> Result<std::path::PathBuf, ToolError> {
        if scopes.is_empty() {
            return Err(denied(
                ToolErrorCode::OutsideWorkspace,
                "this tool has no file access of that kind: the manifest grants no folder",
            ));
        }
        let test_workspace = self.test_workspace.lock().unwrap();
        let ws: &Workspace = match test_workspace.as_ref() {
            Some(ws) => ws,
            None => self.ctx.workspace()?,
        };
        let path = if must_exist {
            ws.resolve_existing(raw)?
        } else {
            ws.resolve_new(raw)?
        };
        for scope in scopes {
            if ws
                .resolve_new(scope)
                .is_ok_and(|root| path.starts_with(root))
            {
                return Ok(path);
            }
        }
        Err(denied(
            ToolErrorCode::OutsideWorkspace,
            format!("path is outside the folders this tool may use: {raw}"),
        ))
    }

    fn fs_read(&self, raw: &str) -> Result<Json, ToolError> {
        let path = self.scoped(&self.perms.fs.read, raw, true)?;
        let meta = std::fs::metadata(&path)
            .map_err(|e| ToolError::new(ToolErrorCode::Io, format!("cannot read {raw}: {e}")))?;
        if !meta.is_file() {
            return Err(ToolError::failed(format!("not a file: {raw}")));
        }
        if meta.len() > MAX_FILE {
            return Err(ToolError::new(
                ToolErrorCode::ResponseTooLarge,
                format!("{raw} is larger than {MAX_FILE} bytes"),
            ));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| {
            ToolError::new(
                ToolErrorCode::Io,
                format!("cannot read {raw} as UTF-8 text: {e}"),
            )
        })?;
        Ok(Json::String(text))
    }

    fn fs_write(&self, raw: &str, text: &str) -> Result<Json, ToolError> {
        let path = self.scoped(&self.perms.fs.write, raw, false)?;
        if text.len() as u64 > MAX_FILE {
            return Err(ToolError::new(
                ToolErrorCode::ResponseTooLarge,
                format!("a write is limited to {MAX_FILE} bytes"),
            ));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolError::new(
                    ToolErrorCode::Io,
                    format!("cannot create the folder for {raw}: {e}"),
                )
            })?;
        }
        std::fs::write(&path, text)
            .map_err(|e| ToolError::new(ToolErrorCode::Io, format!("cannot write {raw}: {e}")))?;
        Ok(json!(text.len()))
    }

    fn fs_list(&self, raw: &str) -> Result<Json, ToolError> {
        let path = self.scoped(&self.perms.fs.read, raw, true)?;
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&path)
            .map_err(|e| ToolError::new(ToolErrorCode::Io, format!("cannot list {raw}: {e}")))?
            .flatten()
        {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            entries.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "dir": kind.is_dir(),
                "size": entry.metadata().map(|m| m.len()).unwrap_or(0),
            }));
            if entries.len() >= MAX_LISTING {
                break;
            }
        }
        entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        Ok(Json::Array(entries))
    }
}

/// Hosts a test registered as reachable although they are on a private address (a local test
/// server). Always `false` outside tests.
#[cfg(test)]
pub(crate) static TRUSTED_TEST_HOSTS: std::sync::Mutex<Vec<String>> =
    std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn trusted_test_host(target: &str) -> bool {
    TRUSTED_TEST_HOSTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|t| t == target)
}

#[cfg(not(test))]
fn trusted_test_host(_target: &str) -> bool {
    false
}

/// One QuickJS runtime with the limits and the host functions of a single call (or of a whole
/// test run).
struct Sandbox {
    rt: Runtime,
    context: Context,
    host: Arc<Host>,
    timed_out: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

fn sandbox(
    perms: &Permissions,
    ctx: &ToolCtx,
    handle: Handle,
    offline: bool,
) -> Result<Sandbox, ToolError> {
    let deadline = Instant::now() + ctx.timeout;
    let timed_out = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let engine =
        |e: rquickjs::Error| ToolError::failed(format!("the JS engine could not start: {e}"));
    let rt = Runtime::new().map_err(engine)?;
    rt.set_memory_limit(MEMORY_LIMIT);
    rt.set_max_stack_size(STACK_LIMIT);
    {
        let (timed_out, stopped, cancel) = (timed_out.clone(), stopped.clone(), ctx.cancel.clone());
        rt.set_interrupt_handler(Some(Box::new(move || {
            if cancel.is_cancelled() {
                stopped.store(true, Ordering::Relaxed);
                return true;
            }
            if Instant::now() >= deadline {
                timed_out.store(true, Ordering::Relaxed);
                return true;
            }
            false
        })));
    }
    let context = Context::full(&rt).map_err(engine)?;
    let host = Arc::new(Host {
        perms: perms.clone(),
        ctx: ctx.clone(),
        handle,
        deadline,
        active_mocks: std::sync::Mutex::new(None),
        test_workspace: std::sync::Mutex::new(None),
        offline,
    });
    Ok(Sandbox {
        rt,
        context,
        host,
        timed_out,
        stopped,
    })
}

impl Sandbox {
    /// Installs `harness` and returns the function that calls `run(args)` (it takes the
    /// arguments as JSON text).
    fn main<'js>(&self, c: &Ctx<'js>) -> rquickjs::Result<Function<'js>> {
        let native = {
            let host = self.host.clone();
            Function::new(
                c.clone(),
                move |op: String, a: String, b: String| -> String {
                    match host.dispatch(&op, &a, &b) {
                        Ok(v) => json!({"ok": v}),
                        Err(e) => {
                            json!({"err": {"code": e.code.as_str(), "message": e.message}})
                        }
                    }
                    .to_string()
                },
            )?
        };
        let make: Function = c.eval(PRELUDE)?;
        make.call((native,))
    }

    /// Ends the run: a stop or an expired deadline wins over what the code returned. Returns
    /// the value with the network target the host functions saw (it outlives the engine).
    fn finish<T>(self, result: Result<T, String>) -> Result<(T, Option<String>), ToolError> {
        let Sandbox {
            rt,
            context,
            host,
            timed_out,
            stopped,
        } = self;
        drop(context);
        drop(rt);
        let target = host.ctx.network_target();
        let fail = |mut e: ToolError| {
            e.network_target = e.network_target.or(target.clone());
            e
        };
        if stopped.load(Ordering::Relaxed) {
            return Err(fail(denied(
                ToolErrorCode::Cancelled,
                "the call was cancelled",
            )));
        }
        if timed_out.load(Ordering::Relaxed) {
            return Err(fail(denied(
                ToolErrorCode::Timeout,
                "the tool did not finish in time and was stopped",
            )));
        }
        match result {
            Ok(v) => Ok((v, target)),
            Err(encoded) => Err(fail(decode(&encoded))),
        }
    }
}

/// Calls the function from [`Sandbox::main`] and waits for its promise.
fn call_main(main: &Function<'_>, args: &Json) -> rquickjs::Result<String> {
    let out: Value = main.call((args.to_string(),))?;
    match out.as_promise() {
        Some(p) => p.finish::<String>(),
        None => out.get::<String>(),
    }
}

/// Runs `code` (the entry file) with `args` and returns the text `run` produced. Blocking: call
/// it from a blocking thread. `handle` is the async runtime the host's network calls run on.
pub fn run_blocking(
    code: &str,
    args: &Json,
    perms: &Permissions,
    ctx: &ToolCtx,
    handle: Handle,
) -> Result<ToolOutput, ToolError> {
    let sb = sandbox(perms, ctx, handle, false)?;
    let result: Result<String, String> = sb.context.with(|c| {
        let attempt = (|| -> rquickjs::Result<String> {
            let main = sb.main(&c)?;
            c.eval::<(), _>(code)?;
            call_main(&main, args)
        })();
        attempt.map_err(|e| describe(&c, e))
    });
    sb.finish(result)
        .map(|(content, network_target)| ToolOutput {
            content,
            network_target,
        })
}

/// Evaluate expectations in QuickJS with the test sandbox's deadline and memory limit.
fn js_regexp(c: &Ctx<'_>, pattern: &Json, text: &str) -> Result<bool, String> {
    let check: Function = c.eval(r#"(encoded, text) => {
        const pattern = JSON.parse(encoded);
        let source, flags = '';
        if (typeof pattern === 'string') {
            if (pattern.startsWith('/')) {
                const end = pattern.lastIndexOf('/');
                if (end <= 0) throw new Error('missing regex delimiter');
                source = pattern.slice(1, end); flags = pattern.slice(end + 1);
            } else source = pattern;
        } else if (pattern && typeof pattern.source === 'string' && typeof pattern.flags === 'string') {
            source = pattern.source; flags = pattern.flags;
        } else throw new Error('matches must be a pattern string or RegExp');
        return new RegExp(source, flags).test(text);
    }"#).map_err(|e| describe(c, e))?;
    check
        .call((pattern.to_string(), text))
        .map_err(|e| describe(c, e))
}

/// Standalone comparison for callers of `judge`; production tests evaluate in their sandbox.
pub(crate) fn match_regexp(pattern: &Json, text: &str) -> Result<bool, String> {
    let runtime = Runtime::new().map_err(|e| e.to_string())?;
    runtime.set_memory_limit(8 * 1024 * 1024);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    runtime.set_interrupt_handler(Some(Box::new(move || {
        std::time::Instant::now() >= deadline
    })));
    let context = Context::full(&runtime).map_err(|e| e.to_string())?;
    context.with(|c| js_regexp(&c, pattern, text))
}

/// How many cases a test file may declare.
pub const MAX_CASES: usize = 20;

/// One case of a test file with what `run` gave for it (`Ok(text)`) or how it failed.
pub type CaseRun = (
    Json,
    Result<String, ToolError>,
    Option<Result<bool, String>>,
);

/// Runs the entry file, then the test file, then every case of the global `cases` array
/// (`[{args, ...}]`) in the one context, so the cases see `run` and anything the test file
/// defines. A failing *case* is data; a test file that does not load, declares no `cases` or
/// more than [`MAX_CASES`] is an `Err`. The whole run shares one deadline (`ctx.timeout`).
pub fn run_cases_blocking(
    code: &str,
    test_code: &str,
    perms: &Permissions,
    ctx: &ToolCtx,
    handle: Handle,
    offline: bool,
) -> Result<Vec<CaseRun>, ToolError> {
    type Raw = Vec<(Json, Result<String, String>, Option<Result<bool, String>>)>;
    let sb = sandbox(perms, ctx, handle, offline)?;
    let result: Result<Raw, String> = sb.context.with(|c| {
        let attempt = (|| -> rquickjs::Result<Result<Raw, String>> {
            let main = sb.main(&c)?;
            if let Err(e) = c.eval::<(), _>(code) {
                let encoded = describe(&c, e);
                let (kind, message) = encoded.split_once('\n').unwrap_or(("", &encoded));
                return Ok(Err(format!("{kind}\nindex.js failed to load (fix the implementation): {message}")));
            }
            if let Err(e) = c.eval::<(), _>(test_code) {
                let encoded = describe(&c, e);
                let (kind, message) = encoded.split_once('\n').unwrap_or(("", &encoded));
                return Ok(Err(format!("{kind}\ntest.js failed to load (fix the test): {message}")));
            }
            let listed: String = c.eval(
                "typeof cases === 'undefined' ? 'null' : (JSON.stringify(cases, (key, value) => key === 'setup' && typeof value === 'function' ? true : key === 'matches' && value instanceof RegExp ? {source: value.source, flags: value.flags} : value) ?? 'null')",
            )?;
            let cases = match serde_json::from_str::<Json>(&listed) {
                Ok(Json::Array(a)) if !a.is_empty() && a.len() <= MAX_CASES => a,
                _ => {
                    return Ok(Err(format!(
                        "\nthe test file must define `const cases = [...]` with 1-{MAX_CASES} cases"
                    )))
                }
            };
            let mut out = Vec::new();
            for case in cases {
                let (_dir, workspace) = match super::package_test::case_workspace(&case) {
                    Ok(value) => value,
                    Err(e) => return Ok(Err(format!("\n{}", e.message))),
                };
                *sb.host.test_workspace.lock().unwrap() = Some(workspace);
                let args = case.get("args").cloned().unwrap_or_else(|| json!({}));
                let http_mocks = case.get("http").and_then(|v| v.as_array()).cloned();
                *sb.host.active_mocks.lock().unwrap() = http_mocks;
                let run = call_main(&main, &args).map_err(|e| describe(&c, e));
                *sb.host.active_mocks.lock().unwrap() = None;
                *sb.host.test_workspace.lock().unwrap() = None;
                let matched = case.get("expect").and_then(|e| e.get("matches")).map(|pattern| {
                    js_regexp(&c, pattern, run.as_deref().unwrap_or_default())
                });
                out.push((case, run, matched));
            }
            Ok(Ok(out))
        })();
        match attempt {
            Ok(r) => r,
            Err(e) => Err(describe(&c, e)),
        }
    });
    let (cases, _) = sb.finish(result)?;
    Ok(cases
        .into_iter()
        .map(|(case, run, matched)| (case, run.map_err(|e| decode(&e)), matched))
        .collect())
}

/// An engine error as `code\nmessage` (the JS exception's own `code` when a host function set
/// one), so it can leave the closure without lifetimes.
fn describe(c: &Ctx<'_>, e: rquickjs::Error) -> String {
    match e {
        rquickjs::Error::Exception => {
            let thrown = c.catch();
            if let Some(obj) = thrown.as_object() {
                let message = obj.get::<_, String>("message").unwrap_or_default();
                let code = obj.get::<_, String>("code").unwrap_or_default();
                let stack_hint = obj.get::<_, String>("stack").unwrap_or_default();
                let message = if message.is_empty() {
                    stack_hint
                } else {
                    message
                };
                return format!("{code}\n{message}");
            }
            let text = thrown
                .as_string()
                .and_then(|s| s.to_string().ok())
                .unwrap_or_else(|| "the code threw a value that is not an error".into());
            format!("\n{text}")
        }
        rquickjs::Error::Allocation => "memory\n".into(),
        other => format!("\n{other}"),
    }
}

fn decode(encoded: &str) -> ToolError {
    let (code, message) = encoded.split_once('\n').unwrap_or(("", encoded));
    if code == "memory" || message.contains("out of memory") {
        return ToolError::failed(format!(
            "the tool used more than {} MiB of memory and was stopped",
            MEMORY_LIMIT / 1024 / 1024
        ));
    }
    if code.is_empty() {
        return ToolError::failed(format!("the tool failed: {message}"));
    }
    ToolError::new(code_from(code), message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::package::FsScope;
    use std::time::Duration;

    fn run(
        code: &str,
        args: Json,
        perms: Permissions,
        timeout: Duration,
    ) -> Result<ToolOutput, ToolError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut ctx = ToolCtx::new(None);
        ctx.timeout = timeout;
        let handle = rt.handle().clone();
        // Like the tool wrapper: the engine runs on a blocking thread of the runtime.
        rt.block_on(async {
            let code = code.to_string();
            let perms = perms.clone();
            tokio::task::spawn_blocking(move || run_blocking(&code, &args, &perms, &ctx, handle))
                .await
                .unwrap()
        })
    }

    fn plain(code: &str, args: Json) -> Result<ToolOutput, ToolError> {
        run(code, args, Permissions::default(), Duration::from_secs(10))
    }

    #[test]
    fn run_receives_the_arguments_and_returns_text_or_json() {
        let out = plain(
            "function run(a) { return a.x + a.y; }",
            json!({"x": 2, "y": 3}),
        )
        .unwrap();
        assert_eq!(out.content, "5");
        let out = plain("function run(a) { return {n: a.n * 2}; }", json!({"n": 4})).unwrap();
        assert_eq!(out.content, r#"{"n":8}"#);
        let out = plain(
            "async function run(a) { return 'hi ' + a.who; }",
            json!({"who": "you"}),
        )
        .unwrap();
        assert_eq!(out.content, "hi you");
        assert_eq!(
            plain("function run() {}", json!({})).unwrap().content,
            "null"
        );
    }

    #[test]
    fn normal_execution_preserves_null_text_and_trailing_whitespace() {
        for code in [
            "function run() { return null; }",
            "function run() {}",
            "async function run() { return null; }",
            "async function run() {}",
        ] {
            assert_eq!(plain(code, json!({})).unwrap().content, "null");
        }
        assert_eq!(
            plain("function run() { return ''; }", json!({}))
                .unwrap()
                .content,
            ""
        );
        assert_eq!(
            plain("function run() { return '{}\\n '; }", json!({}))
                .unwrap()
                .content,
            "{}\n "
        );
    }

    #[test]
    fn base64_supports_ascii_and_latin1_and_rejects_invalid_input() {
        for (text, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("\u{ff}\0", "/wA="),
        ] {
            let out = plain(
                "function run(a) { return [btoa(a.text), atob(a.encoded)]; }",
                json!({"text": text, "encoded": encoded}),
            )
            .unwrap();
            assert_eq!(out.content, json!([encoded, text]).to_string());
        }
        assert_eq!(
            plain("function run() { return atob(' Zm8\\n'); }", json!({}))
                .unwrap()
                .content,
            "fo"
        );
        for input in [
            "A",
            "Zg=",
            "Z===",
            "Zm$8",
            "Zg===",
            "Zm8=extra",
            "Zg\u{a0}==",
        ] {
            let e = plain(
                "function run(a) { return atob(a.text); }",
                json!({"text": input}),
            )
            .unwrap_err();
            assert!(e.message.contains("Invalid base64"), "{input}: {e:?}");
        }
        assert!(plain("function run() { return btoa('🙂'); }", json!({})).is_err());
    }

    #[test]
    fn utf8_roundtrips_unicode_through_base64_and_buffer_views() {
        let code = r#"function run(a) {
            const bytes = new TextEncoder().encode(a.text);
            const encoded = btoa(Array.from(bytes, b => String.fromCharCode(b)).join(''));
            const decoded = Uint8Array.from(atob(encoded), c => c.charCodeAt(0));
            const padded = new Uint8Array(decoded.length + 2);
            padded.set(decoded, 1);
            return [encoded, new TextDecoder().decode(padded.subarray(1, padded.length - 1)),
              new TextDecoder().decode(bytes.buffer)];
        }"#;
        for text in [
            "ASCII",
            "Türkçe 🙂",
            "\u{7f}\u{80}\u{7ff}\u{800}\u{ffff}\u{10000}\u{10ffff}",
        ] {
            let out = plain(code, json!({"text": text})).unwrap();
            let actual: Json = serde_json::from_str(&out.content).unwrap();
            assert_eq!(actual[1], text);
            assert_eq!(actual[2], text);
            if text == "Türkçe 🙂" {
                assert_eq!(actual[0], "VMO8cmvDp2Ug8J+Zgg==");
            }
        }
    }

    #[test]
    fn utf8_handles_replacement_bom_and_fatal_errors() {
        let out = plain(r#"function run() {
            const d = new TextDecoder();
            return [d.decode(), new TextEncoder().encode().length,
              Array.from(new TextEncoder().encode('\ud800')),
              d.decode(new Uint8Array([0xe2, 0x82, 0x41, 0xff])),
              d.decode(new Uint8Array([0xed, 0xa0, 0x80])),
              d.decode(new Uint8Array([0xef, 0xbb, 0xbf, 65])),
              new TextDecoder('utf8', {ignoreBOM: true}).decode(new Uint8Array([0xef, 0xbb, 0xbf, 65]))];
        }"#, json!({})).unwrap();
        assert_eq!(
            out.content,
            json!(["", 0, [239, 191, 189], "�A�", "���", "A", "\u{feff}A"]).to_string()
        );
        for code in [
            "function run() { return new TextDecoder('utf-8', {fatal: true}).decode(new Uint8Array([0xc0])); }",
            "function run() { return new TextDecoder('utf-8', {fatal: true}).decode(new Uint8Array([0xe2, 0x82])); }",
            "function run() { return new TextDecoder('latin1'); }",
            "function run() { return new TextDecoder().decode('abc'); }",
            "function run() { return new TextDecoder().decode(new Uint8Array(), {stream: true}); }",
        ] { assert!(plain(code, json!({})).is_err(), "{code}"); }
    }

    #[test]
    fn prelude_preserves_existing_web_globals() {
        let rt = Runtime::new().unwrap();
        let context = Context::full(&rt).unwrap();
        context.with(|c| {
            c.eval::<(), _>("globalThis.saved = {}; for (const n of ['btoa', 'atob', 'TextEncoder', 'TextDecoder']) { saved[n] = globalThis[n] = function native() {}; }").unwrap();
            let make: Function = c.eval(PRELUDE).unwrap();
            let host = Function::new(c.clone(), || "").unwrap();
            let _: Function = make.call((host,)).unwrap();
            assert!(c.eval::<bool, _>("Object.keys(saved).every(n => saved[n] === globalThis[n])").unwrap());
        });
    }

    #[test]
    fn the_code_has_no_ambient_access() {
        let probe = r#"function run() {
            const names = ['fetch', 'XMLHttpRequest', 'require', 'process', 'std', 'os', 'Deno', 'setTimeout', 'WebSocket', 'import', '__host'];
            return names.filter((n) => typeof globalThis[n] !== 'undefined').join(',');
        }"#;
        assert_eq!(plain(probe, json!({})).unwrap().content, "");
        let e = plain("function run() { return import('os'); }", json!({})).unwrap_err();
        assert_eq!(e.code, ToolErrorCode::Failed);
        // `harness` cannot be replaced from inside.
        let out = plain(
            "function run() { try { globalThis.harness = 1; } catch (e) {} return typeof harness.http.get; }",
            json!({}),
        )
        .unwrap();
        assert_eq!(out.content, "function");
    }

    #[test]
    fn errors_are_reported_in_words() {
        let e = plain("function run() { throw new Error('boom'); }", json!({})).unwrap_err();
        assert_eq!(
            (e.code, e.message.as_str()),
            (ToolErrorCode::Failed, "the tool failed: boom")
        );
        let e = plain("function run( {", json!({})).unwrap_err();
        assert_eq!(e.code, ToolErrorCode::Failed);
        let e = plain("let x = 1;", json!({})).unwrap_err();
        assert!(e.message.contains("function run"), "{}", e.message);
    }

    #[test]
    fn an_endless_loop_is_stopped_at_the_deadline() {
        let started = Instant::now();
        let e = run(
            "function run() { while (true) {} }",
            json!({}),
            Permissions::default(),
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert_eq!(e.code, ToolErrorCode::Timeout);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_greedy_script_hits_the_heap_limit() {
        let e = plain(
            "function run() { const a = []; for (;;) a.push('x'.repeat(1 << 20) + a.length); }",
            json!({}),
        )
        .unwrap_err();
        assert!(e.message.contains("memory"), "{}", e.message);
    }

    #[test]
    fn a_cancelled_call_stops() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        let ctx = ToolCtx::new(None);
        let cancel = ctx.cancel.clone();
        let handle = rt.handle().clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            cancel.cancel();
        });
        let e = rt
            .block_on(async {
                tokio::task::spawn_blocking(move || {
                    run_blocking(
                        "function run() { for(;;) {} }",
                        &json!({}),
                        &Permissions::default(),
                        &ctx,
                        handle,
                    )
                })
                .await
                .unwrap()
            })
            .unwrap_err();
        assert_eq!(e.code, ToolErrorCode::Cancelled);
    }

    #[test]
    fn host_functions_refuse_what_the_manifest_does_not_grant() {
        let e = plain(
            "function run() { return harness.fs.read('a.txt'); }",
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.code, ToolErrorCode::OutsideWorkspace);
        let e = plain(
            "function run() { return harness.http.get('http://example.com/'); }",
            json!({}),
        )
        .unwrap_err();
        assert_eq!(e.code, ToolErrorCode::Network);
        assert!(e.message.contains("allowed hosts"), "{}", e.message);
        let perms = Permissions {
            fs: FsScope {
                read: vec![".".into()],
                write: vec![],
            },
            ..Default::default()
        };
        // Granted for reading, but there is no workspace for this call.
        let e = run(
            "function run() { return harness.fs.read('a.txt'); }",
            json!({}),
            perms,
            Duration::from_secs(5),
        )
        .unwrap_err();
        assert_eq!(e.code, ToolErrorCode::NoWorkspace);
    }
}
