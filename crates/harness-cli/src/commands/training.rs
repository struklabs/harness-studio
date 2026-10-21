//! Persisted training jobs through the owning instance's Control API.
use crate::cli::TrainingCommand;
use crate::client::ApiClient;
use crate::error::{CliError, CliResult};
use crate::output::Out;
use serde_json::{json, Value};

pub async fn run(client: &ApiClient, command: &TrainingCommand, out: &Out) -> CliResult<()> {
    let value = match command {
        TrainingCommand::List => client.get("/training").await?,
        TrainingCommand::Status { id } => client.get(&format!("/training/{id}")).await?,
        TrainingCommand::Start { spec } => {
            let bytes =
                std::fs::read(spec).map_err(|e| CliError::general("cli.io", e.to_string()))?;
            let request: Value = serde_json::from_slice(&bytes)
                .map_err(|e| CliError::usage("cli.invalid_argument", e.to_string()))?;
            let job = client.post("/training", &request).await?;
            let id = job["id"].as_u64().ok_or_else(|| {
                CliError::general("api.unexpected_response", "training response has no id")
            })?;
            client
                .post(&format!("/training/{id}/start"), &json!({}))
                .await
                .map_err(|e| e.with("job_id", id))?
        }
        TrainingCommand::Cancel { id } => {
            client
                .post(&format!("/training/{id}/cancel"), &json!({}))
                .await?;
            json!({"id": id, "cancel_requested": true})
        }
        TrainingCommand::Resume { id } => {
            client
                .post(&format!("/training/{id}/resume"), &json!({}))
                .await?
        }
        TrainingCommand::Export {
            id,
            name,
            gguf,
            quant,
            adapter_only,
        } => client
            .post(
                &format!("/training/{id}/export"),
                &json!({"name": name, "gguf": gguf, "quant": quant, "adapter_only": adapter_only}),
            )
            .await?,
    };
    out.data(&value, || {
        serde_json::to_string_pretty(&value).unwrap_or_default()
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        http::{HeaderMap, Method, Uri},
        routing::any,
        Json, Router,
    };
    use clap::Parser;
    use std::sync::{Arc, Mutex};

    #[tokio::test]
    async fn lifecycle_commands_use_the_owning_api_and_forward_export_options() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = seen.clone();
        let app = Router::new().fallback(any(
            move |method: Method, uri: Uri, headers: HeaderMap, bytes: Bytes| {
                let captured = captured.clone();
                async move {
                    assert_eq!(headers["authorization"], "Bearer training-test");
                    let body: Value = if bytes.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes).unwrap()
                    };
                    captured.lock().unwrap().push((
                        method.to_string(),
                        uri.path().to_string(),
                        body,
                    ));
                    Json(json!({"id": 42}))
                }
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = ApiClient::new(
            format!("http://{addr}/api/harness/v1"),
            "training-test".into(),
        );
        let out = Out {
            json: true,
            quiet: true,
            yes: true,
        };
        let dir = tempfile::tempdir().unwrap();
        let spec = dir.path().join("spec.json");
        std::fs::write(
            &spec,
            r#"{"model":"/models/smol","dataset":"/data.jsonl","steps":5}"#,
        )
        .unwrap();
        for command in [
            TrainingCommand::List,
            TrainingCommand::Start { spec },
            TrainingCommand::Status { id: 42 },
            TrainingCommand::Cancel { id: 42 },
            TrainingCommand::Resume { id: 42 },
            TrainingCommand::Export {
                id: 42,
                name: Some("adapter".into()),
                gguf: true,
                quant: "Q5_K_M".into(),
                adapter_only: true,
            },
        ] {
            run(&client, &command, &out).await.unwrap();
        }
        server.abort();
        let seen = seen.lock().unwrap();
        let routes: Vec<_> = seen.iter().map(|(m, p, _)| format!("{m} {p}")).collect();
        assert_eq!(
            routes,
            [
                "GET /api/harness/v1/training",
                "POST /api/harness/v1/training",
                "POST /api/harness/v1/training/42/start",
                "GET /api/harness/v1/training/42",
                "POST /api/harness/v1/training/42/cancel",
                "POST /api/harness/v1/training/42/resume",
                "POST /api/harness/v1/training/42/export",
            ]
        );
        assert_eq!(seen[1].2["steps"], 5);
        assert_eq!(
            seen[6].2,
            json!({"name":"adapter", "gguf":true, "quant":"Q5_K_M", "adapter_only":true})
        );
    }

    #[test]
    fn parser_limits_quantization_and_accepts_all_training_actions() {
        use crate::cli::{Cli, Command};
        for args in [
            vec!["list"],
            vec!["start", "spec.json"],
            vec!["status", "42"],
            vec!["cancel", "42"],
            vec!["resume", "42"],
            vec![
                "export",
                "42",
                "--adapter-only",
                "--gguf",
                "--quant",
                "Q8_0",
            ],
        ] {
            let argv: Vec<_> = ["harness", "training"].into_iter().chain(args).collect();
            assert!(matches!(
                Cli::try_parse_from(argv).unwrap().command,
                Command::Training { .. }
            ));
        }
        assert!(
            Cli::try_parse_from(["harness", "training", "export", "42", "--quant", "INVALID"])
                .is_err()
        );
    }
}
