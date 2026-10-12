//! `harness mcp`: the Harness MCP server over stdio.
//!
//! Tools talk to a Control API exactly like the other commands ([`crate::session`]): the
//! running owner's (re-discovered on every call, so a GUI started or restarted later is picked
//! up and a rotated session token is re-read), a remote one (`--host`), or with `--headless` a
//! temporary in-process instance that lives until stdin closes (its servers stop then).
//!
//! The session token of a local owner is full access, so the MCP layer narrows it: `--scope`
//! (default `read,servers:write,tools`) decides which tools are listed and callable. Downloads
//! and deletions need `--scope read,servers:write,tools,models:write`. The `tools` scope only
//! matters once the user turns on Settings > Tools > "Offer tools to MCP clients".

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_mcp::{Backend, BackendError, BackendResult, BoxFuture, McpServer, Method, Transport};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::cli::{GlobalArgs, McpArgs};
use crate::client::ApiClient;
use crate::error::{CliError, CliResult};
use crate::output::Out;
use crate::session::{connect, Need, Session, Via};

struct CliBackend {
    global: GlobalArgs,
    data_dir: PathBuf,
    /// A remote or temporary session is kept for the process lifetime; an owner's API is
    /// re-discovered per call.
    kept: Mutex<Option<Session>>,
}

fn to_backend_error(e: CliError) -> BackendError {
    let mut b = BackendError::new(e.code, e.message);
    b.params = e.params;
    b.status = b
        .params
        .get("http_status")
        .and_then(Value::as_u64)
        .map(|s| s as u16);
    b
}

impl CliBackend {
    async fn client(&self) -> BackendResult<ApiClient> {
        let mut kept = self.kept.lock().await;
        if let Some(s) = kept.as_ref() {
            return Ok(s.client.clone());
        }
        let quiet = Out {
            json: true,
            quiet: true,
            yes: true,
        };
        let session = connect(&self.global, &self.data_dir, Need::Servers, &quiet)
            .await
            .map_err(|e| {
                let mut b = to_backend_error(e);
                if b.code == "cli.no_instance" {
                    b.message = "No Harness instance is running. Start Harness Studio with \
Settings > Developer API on, or `harness serve`, or run `harness mcp --headless`."
                        .into();
                }
                b
            })?;
        let client = session.client.clone();
        if session.via != Via::Owner {
            *kept = Some(session);
        }
        Ok(client)
    }

    async fn close(&self) {
        if let Some(s) = self.kept.lock().await.take() {
            s.close().await;
        }
    }
}

impl Backend for CliBackend {
    fn request<'a>(
        &'a self,
        method: Method,
        path: &'a str,
        body: Option<Value>,
    ) -> BoxFuture<'a, BackendResult<Value>> {
        Box::pin(async move {
            let client = self.client().await?;
            let m = match method {
                Method::Get => reqwest::Method::GET,
                Method::Post => reqwest::Method::POST,
                Method::Delete => reqwest::Method::DELETE,
            };
            client
                .request(m, path, body.as_ref())
                .await
                .map_err(to_backend_error)
        })
    }

    fn client_endpoint(&self, endpoint: &str) -> String {
        match self.kept.try_lock() {
            Ok(kept) => kept
                .as_ref()
                .map(|s| s.reachable_endpoint(endpoint))
                .unwrap_or_else(|| endpoint.to_string()),
            Err(_) => endpoint.to_string(),
        }
    }
}

pub async fn run(args: &McpArgs, g: &GlobalArgs, data_dir: &Path) -> CliResult<()> {
    let scopes = harness_mcp::parse_scopes(&args.scope)
        .map_err(|e| CliError::usage("cli.invalid_argument", format!("--scope: {}", e)))?;
    let backend = Arc::new(CliBackend {
        global: g.clone(),
        data_dir: data_dir.to_path_buf(),
        kept: Mutex::new(None),
    });
    eprintln!(
        "[harness mcp] ready (stdio, scopes: {})",
        scopes
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    let server = McpServer::new(backend.clone(), scopes, Transport::Stdio);
    harness_mcp::stdio::serve(server, tokio::io::stdin(), tokio::io::stdout()).await;
    backend.close().await;
    Ok(())
}
