//! A small real HTTP/1.1 server for the tests of the MCP client's HTTP transports and OAuth
//! flow: it listens on 127.0.0.1, hands each request to a handler and writes what the handler
//! returns, including long-lived event streams. Test code only; the product code under test
//! talks to it over real sockets.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct Req {
    pub method: String,
    /// Path and query as sent.
    pub target: String,
    /// Lower-case header names.
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Req {
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// A query or form parameter.
    pub fn param(&self, key: &str) -> Option<String> {
        let query = self.target.split_once('?').map_or("", |(_, q)| q);
        let from_url = reqwest::Url::parse(&format!("http://x/?{query}")).ok()?;
        let hit = from_url
            .query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned());
        hit.or_else(|| {
            let form = reqwest::Url::parse(&format!("http://x/?{}", self.body)).ok()?;
            let v = form
                .query_pairs()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned());
            v
        })
    }
}

pub enum Resp {
    Full {
        status: u16,
        headers: Vec<(String, String)>,
        body: String,
    },
    /// `text/event-stream` that stays open until the sender is dropped.
    Stream(mpsc::UnboundedReceiver<String>),
}

impl Resp {
    pub fn json(status: u16, body: &serde_json::Value) -> Self {
        Resp::Full {
            status,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: body.to_string(),
        }
    }

    pub fn status(status: u16) -> Self {
        Resp::Full {
            status,
            headers: vec![],
            body: String::new(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        if let Resp::Full { headers, .. } = &mut self {
            headers.push((name.into(), value.into()));
        }
        self
    }
}

pub struct TestServer {
    /// `http://127.0.0.1:<port>`
    pub base: String,
    /// Every request seen: `(method, target)`.
    pub seen: Arc<Mutex<Vec<(String, String)>>>,
    task: JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    pub fn count(&self, method: &str, path: &str) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(m, t)| m == method && t.split('?').next() == Some(path))
            .count()
    }
}

/// Starts the server. The handler gets the server's own base URL (so it can point to itself).
pub async fn serve<F>(handler: F) -> TestServer
where
    F: Fn(&str, Req) -> Resp + Send + Sync + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let task = {
        let (base, seen) = (base.clone(), seen.clone());
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let (handler, base, seen) = (handler.clone(), base.clone(), seen.clone());
                tokio::spawn(async move {
                    let _ = handle(stream, handler, base, seen).await;
                });
            }
        })
    };
    TestServer { base, seen, task }
}

async fn handle<F>(
    mut stream: tokio::net::TcpStream,
    handler: Arc<F>,
    base: String,
    seen: Arc<Mutex<Vec<(String, String)>>>,
) -> std::io::Result<()>
where
    F: Fn(&str, Req) -> Resp + Send + Sync + 'static,
{
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split(' ');
    let method = first.next().unwrap_or("").to_string();
    let target = first.next().unwrap_or("").to_string();
    let headers: HashMap<String, String> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let want: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < want {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    seen.lock().unwrap().push((method.clone(), target.clone()));
    let req = Req {
        method,
        target,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    };
    match handler(&base, req) {
        Resp::Full {
            status,
            headers,
            body,
        } => {
            let mut out = format!("HTTP/1.1 {status} X\r\n");
            for (k, v) in headers {
                out.push_str(&format!("{k}: {v}\r\n"));
            }
            out.push_str(&format!(
                "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ));
            stream.write_all(out.as_bytes()).await?;
            stream.shutdown().await
        }
        Resp::Stream(mut rx) => {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
                )
                .await?;
            while let Some(piece) = rx.recv().await {
                stream.write_all(piece.as_bytes()).await?;
                stream.flush().await?;
            }
            stream.shutdown().await
        }
    }
}
