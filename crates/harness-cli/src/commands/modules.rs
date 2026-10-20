//! `modules ls|export|import|rm`: optional modules and their offline `.hsmod` bundles.

use std::path::Path;

use serde_json::{json, Value};

use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult};
use crate::output::{bytes, str_of, table, Out};

/// The Control API runs on this machine for every local command; relative paths are resolved here.
fn absolute(path: &Path) -> CliResult<String> {
    std::path::absolute(path)
        .map(|p| p.display().to_string())
        .map_err(|e| CliError::general("cli.io", e.to_string()))
}

pub async fn ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let list = client.get("/modules").await?;
    out.data(&list, || {
        let rows: Vec<Vec<String>> = list
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|v| {
                let status = if v["manifest"]["planned"] == true {
                    "planned".to_string()
                } else {
                    str_of(&v["state"]["status"])
                };
                vec![
                    str_of(&v["manifest"]["id"]),
                    v["state"]["installed_version"]
                        .as_str()
                        .unwrap_or("-")
                        .to_string(),
                    str_of(&v["manifest"]["version"]),
                    status,
                ]
            })
            .collect();
        table(&["ID", "INSTALLED", "BUNDLED", "STATUS"], &rows)
    });
    Ok(())
}

pub async fn export(client: &ApiClient, id: &str, file: &Path, out: &Out) -> CliResult<()> {
    let report = client
        .post(
            &format!("/modules/{}/export", seg(id)),
            &json!({"path": absolute(file)?}),
        )
        .await?;
    out.data(&report, || summary_export(&report));
    Ok(())
}

fn summary_export(report: &Value) -> String {
    format!(
        "Exported {id} {version} to {path}\n{files} files, {size}\nsha256 {sha}\nTo make this bundle installable, record that hash for {id} {version} in crates/harness-core/src/modules/official_bundles.json.",
        id = str_of(&report["id"]),
        version = str_of(&report["version"]),
        path = str_of(&report["path"]),
        files = report["files"],
        size = bytes(report["bytes"].as_u64().unwrap_or(0)),
        sha = str_of(&report["sha256"]),
    )
}

pub async fn import(client: &ApiClient, file: &Path, out: &Out) -> CliResult<()> {
    let report = client
        .post("/modules/import", &json!({"path": absolute(file)?}))
        .await?;
    out.data(&report, || {
        let list = |k: &str| {
            let items: Vec<String> = report[k]
                .as_array()
                .map(|a| a.iter().map(str_of).collect())
                .unwrap_or_default();
            if items.is_empty() {
                "none".to_string()
            } else {
                items.join(", ")
            }
        };
        format!(
            "Installed {} {} from the bundle (no network used)\nInstalled components: {}\nShared components reused: {}",
            str_of(&report["id"]),
            str_of(&report["version"]),
            list("installed_components"),
            list("reused_components"),
        )
    });
    Ok(())
}

pub async fn rm(client: &ApiClient, id: &str, out: &Out) -> CliResult<()> {
    out.confirm(&format!("Remove module {}?", id))?;
    client.delete(&format!("/modules/{}", seg(id))).await?;
    let value = json!({"removed": id});
    out.data(&value, || format!("Removed module {}", id));
    Ok(())
}
