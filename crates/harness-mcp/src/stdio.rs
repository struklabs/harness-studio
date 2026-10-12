//! stdio transport: newline-delimited JSON-RPC on stdin/stdout. Only MCP messages go to
//! stdout; requests run concurrently (a long `load_model` does not block `tools/list`);
//! `notifications/cancelled` aborts the named request; EOF on stdin ends the loop.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::protocol::{error_reply, PARSE_ERROR};
use crate::McpServer;

async fn write_line<W: AsyncWrite + Unpin>(out: &Mutex<W>, value: &Value) {
    let mut line = serde_json::to_string(value).unwrap_or_default();
    line.push('\n');
    let mut w = out.lock().await;
    let _ = w.write_all(line.as_bytes()).await;
    let _ = w.flush().await;
}

fn id_key(id: &Value) -> String {
    id.to_string()
}

/// Serves MCP until `input` ends. Returns after every in-flight request finished (or was
/// cancelled).
pub async fn serve<R, W>(server: McpServer, input: R, output: W)
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let out = Arc::new(Mutex::new(output));
    let tasks: Arc<Mutex<HashMap<String, JoinHandle<()>>>> = Arc::default();
    let mut lines = BufReader::new(input).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let reply = error_reply(
                    &Value::Null,
                    PARSE_ERROR,
                    &format!("Parse error: {}", e),
                    None,
                    400,
                );
                write_line(&out, &reply.body).await;
                continue;
            }
        };
        if msg["method"] == "notifications/cancelled" {
            let key = id_key(&msg["params"]["requestId"]);
            if let Some(task) = tasks.lock().await.remove(&key) {
                task.abort();
            }
            continue;
        }
        let server = server.clone();
        let out = out.clone();
        let key = msg.get("id").map(id_key);
        let tasks_done = tasks.clone();
        let done_key = key.clone();
        let task = tokio::spawn(async move {
            if let Some(reply) = server.handle(&msg).await {
                write_line(&out, &reply.body).await;
            }
            if let Some(k) = done_key {
                tasks_done.lock().await.remove(&k);
            }
        });
        if let Some(k) = key {
            tasks.lock().await.insert(k, task);
        }
    }
    let pending: Vec<JoinHandle<()>> = tasks.lock().await.drain().map(|(_, t)| t).collect();
    for t in pending {
        let _ = t.await;
    }
}
