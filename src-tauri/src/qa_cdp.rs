//! Opt-in isolated debug fixture transport for WebView2's real CDP API.
//! No command mocks: every method executes through CallDevToolsProtocolMethod on the UI thread.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, CoTaskMemPWSTR,
    DevToolsProtocolEventReceivedEventHandler,
};
use windows_core::{HSTRING, PWSTR};

pub fn start(window: tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
    let Ok(token) = std::env::var("HARNESS_QA_CDP_TOKEN") else {
        return Ok(());
    };
    if token.len() < 32 {
        return Err("QA CDP requires a random token".into());
    }
    let root = std::path::PathBuf::from(std::env::var("HARNESS_DATA_DIR")?).canonicalize()?;
    let scratch = std::env::current_dir()?.join("scratch").canonicalize()?;
    if !root.starts_with(&scratch) || root == scratch {
        return Err("QA CDP requires scratch data".into());
    }
    let port = std::env::var("HARNESS_QA_CDP_PORT")
        .ok()
        .map(|value| value.parse::<u16>())
        .transpose()?;
    let listener = match port {
        Some(port) => std::net::TcpListener::bind(("127.0.0.1", port))?,
        None => harness_core::ports::ISOLATED_AUTO_RANGE
            .find_map(|port| std::net::TcpListener::bind(("127.0.0.1", port)).ok())
            .ok_or("no free isolated QA CDP port")?,
    };
    let timeout = Duration::from_millis(
        std::env::var("HARNESS_QA_CDP_TIMEOUT_MS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()?
            .unwrap_or(20_000),
    );
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let capture = events.clone();
    window.with_webview(move |webview| {
        // SAFETY: with_webview and callbacks execute on the WebView's COM apartment.
        unsafe {
            let core = webview.controller().CoreWebView2().unwrap();
            for method in ["Runtime.exceptionThrown", "Runtime.consoleAPICalled"] {
                let receiver = core
                    .GetDevToolsProtocolEventReceiver(&HSTRING::from(method))
                    .unwrap();
                let capture = capture.clone();
                let handler =
                    DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
                        if let Some(args) = args {
                            let mut raw = PWSTR::null();
                            args.ParameterObjectAsJson(&mut raw)?;
                            let guard = CoTaskMemPWSTR::from(raw);
                            let text = guard.to_string();
                            if let Ok(params) = serde_json::from_str::<Value>(&text) {
                                capture
                                    .lock()
                                    .unwrap()
                                    .push(json!({"method":method,"params":params}));
                            }
                        }
                        Ok(())
                    }));
                let mut event_token = Default::default();
                receiver
                    .add_DevToolsProtocolEventReceived(&handler, &mut event_token)
                    .unwrap();
            }
        }
    })?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let result = serve(stream, &window, &events, &token, timeout);
            if let Err(e) = result {
                eprintln!("QA CDP: {e}");
            }
        }
    });
    Ok(())
}

fn serve(
    mut stream: std::net::TcpStream,
    window: &tauri::WebviewWindow,
    events: &Mutex<Vec<Value>>,
    token: &str,
    timeout: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let route = line.split_whitespace().nth(1).unwrap_or("").to_string();
    let mut length = 0;
    let mut authorized = false;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>()?;
            }
            if name.eq_ignore_ascii_case("authorization") {
                authorized = value.trim() == format!("Bearer {token}");
            }
        }
    }
    if !authorized || length > 4 * 1024 * 1024 {
        return Err("Invalid QA CDP request".into());
    }
    let result = if route == "/json/list" {
        json!([{"type":"page","url":window.url()?.as_str(),"transport":"webview2-api"}])
    } else if route == "/activate-offscreen" {
        // Real visibility is needed by background polling. Never show a taskbar button or
        // cover the user's screen, and never request focus.
        window.set_skip_taskbar(true)?;
        window.set_position(tauri::PhysicalPosition::new(-20000, -20000))?;
        // SAFETY: this is the live isolated window's HWND. SW_SHOWNOACTIVATE restores
        // visibility without stealing keyboard focus from the user's current app.
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                windows::Win32::Foundation::HWND(window.hwnd()?.0),
                windows::Win32::UI::WindowsAndMessaging::SW_SHOWNOACTIVATE,
            );
        }
        json!({"offscreen":true})
    } else if route == "/cdp" {
        let mut body = vec![0; length];
        reader.read_exact(&mut body)?;
        let request: Value = serde_json::from_slice(&body)?;
        let method = request["method"]
            .as_str()
            .ok_or("Missing method")?
            .to_string();
        if !["Runtime.", "Page.", "Input.", "Emulation."]
            .iter()
            .any(|prefix| method.starts_with(prefix))
        {
            return Err("Unsupported QA CDP domain".into());
        }
        let params = request
            .get("params")
            .cloned()
            .unwrap_or_else(|| json!({}))
            .to_string();
        let (tx, rx) = mpsc::channel();
        window.with_webview(move |webview| {
            // SAFETY: the COM method runs on the owning UI thread. Strings live through the call.
            unsafe {
                let core = webview.controller().CoreWebView2().unwrap();
                let completion_tx = tx.clone();
                let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(
                    move |status, result| {
                        let value = status.map_err(|e| e.to_string()).and_then(|()| {
                            serde_json::from_str::<Value>(&result).map_err(|e| e.to_string())
                        });
                        let _ = completion_tx.send(value);
                        Ok(())
                    },
                ));
                if let Err(e) = core.CallDevToolsProtocolMethod(
                    &HSTRING::from(method),
                    &HSTRING::from(params),
                    &handler,
                ) {
                    let _ = tx.send(Err(e.to_string()));
                }
            }
        })?;
        match rx.recv_timeout(timeout)? {
            Ok(result) => {
                json!({"result":result,"events":std::mem::take(&mut *events.lock().unwrap())})
            }
            Err(error) => json!({"error":error}),
        }
    } else {
        return Err("Unknown QA CDP route".into());
    };
    let body = result.to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body)?;
    Ok(())
}
