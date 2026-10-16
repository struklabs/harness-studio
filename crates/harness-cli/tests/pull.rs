//! Real CLI exit codes against a disposable loopback Control API; no app or models.
mod common;

use axum::{
    routing::{get, post},
    Json, Router,
};
use harness_core::error_key::err;
use serde_json::{json, Value};

#[tokio::test]
async fn pull_and_companion_failures_exit_nonzero() {
    for failed_job in [Some(1), Some(2), None] {
        let error = err("downloads.interrupted")
            .with("file", "weights.gguf")
            .with("error", "http.connection_closed")
            .to_string();
        let app = Router::new()
            .route("/api/harness/v1/models/pull", post(|| async {
                Json(json!({"job_id": 1, "variant": {"name": "fixture"}, "companions": [2]}))
            }))
            .route("/api/harness/v1/downloads/{id}", get(move |axum::extract::Path(id): axum::extract::Path<u64>| {
                let error = error.clone();
                async move {
                    let status: Value = if failed_job == Some(id) { json!({"Error": error}) } else { json!("Completed") };
                    Json(json!({
                        "id": id, "repo_id": "owner/repo", "variant_name": "fixture",
                        "total_bytes": 32, "downloaded_bytes": 12, "current_shard_name": "weights.gguf",
                        "current_shard_index": 1, "total_shards": 1, "speed_bytes_per_sec": 0,
                        "status": status
                    }))
                }
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let host = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let result = tokio::task::spawn_blocking(move || {
            let dir = tempfile::tempdir().unwrap();
            common::run(
                dir.path(),
                &[
                    "--host",
                    &host,
                    "--token",
                    "test",
                    "--json",
                    "pull",
                    "owner/repo",
                ],
            )
        })
        .await
        .unwrap();
        if let Some(id) = failed_job {
            assert_ne!(result.code, 0, "{}", result.stderr);
            let error = result.error();
            assert_eq!(error["code"], "downloads.interrupted");
            assert_eq!(error["exit_code"], result.code);
            assert_eq!(error["params"]["job_id"], id);
            let message = error["message"].as_str().unwrap();
            assert!(message.contains("weights.gguf"), "{message}");
            assert!(message.contains("connection dropped"), "{message}");
            assert!(
                message.contains("harness pull") && message.contains("resume"),
                "{message}"
            );
            assert!(result.stdout.is_empty(), "{}", result.stdout);
        } else {
            assert_eq!(result.code, 0, "{}", result.stderr);
            assert_eq!(result.json()["download"]["status"], "Completed");
        }
        server.abort();
        let _ = server.await;
    }
}
