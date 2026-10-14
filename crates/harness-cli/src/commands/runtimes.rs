//! `runtime ls|add|rm|verify|suggest`: the runtime registry through the Control API.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};

use crate::client::{seg, ApiClient};
use crate::error::{CliError, CliResult};
use crate::output::{bytes, str_of, table, Out};

fn version_text(v: &Value) -> String {
    let version = &v["version"];
    match (version["build"].as_u64(), version["commit"].as_str()) {
        (Some(b), Some(c)) => format!("b{} ({})", b, c),
        (Some(b), None) => format!("b{}", b),
        _ => version["raw"].as_str().unwrap_or("-").to_string(),
    }
}

fn source_text(v: &Value) -> String {
    let src = &v["entry"]["source"];
    match src["kind"].as_str() {
        Some("github") => format!("{} {}", str_of(&src["repo"]), str_of(&src["tag"])),
        Some("local") => {
            let copied = if src["copied"] == true { " (copy)" } else { "" };
            format!("{}{}", str_of(&src["path"]), copied)
        }
        _ if v["kind"] == "system" => "found on PATH / WinGet".into(),
        _ => {
            let o = &v["official"];
            match (o["source"].as_str(), o["fallback"] == true) {
                (Some(src), true) => format!(
                    "Settings > Runtime ({} configured, {} used)",
                    str_of(&o["configured_mode"]),
                    src
                ),
                (Some(src), false) => format!("Settings > Runtime ({})", src),
                _ => "Settings > Runtime".into(),
            }
        }
    }
}

pub async fn ls(client: &ApiClient, out: &Out) -> CliResult<()> {
    let list = client.get("/runtimes").await?;
    out.data(&list, || {
        let rows: Vec<Vec<String>> = list
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|v| {
                let name = v["name"].as_str().filter(|n| !n.is_empty());
                let fallback = if v["kind"] == "system" {
                    "system llama-server"
                } else {
                    "llama.cpp (official)"
                };
                vec![
                    str_of(&v["id"]),
                    name.unwrap_or(fallback).to_string(),
                    version_text(v),
                    v["backend"].as_str().unwrap_or("-").to_string(),
                    if v["available"] == true {
                        format!(
                            "{} flags, {} params hidden",
                            v["flag_count"],
                            v["unsupported_params"].as_array().map_or(0, Vec::len)
                        )
                    } else {
                        v["error"]
                            .as_str()
                            .map_or_else(|| "unavailable".to_string(), crate::messages::core_text)
                    },
                    v["entry"]["trust"].as_str().unwrap_or("-").to_string(),
                    source_text(v),
                ]
            })
            .collect();
        let mut text = table(
            &[
                "ID",
                "NAME",
                "BUILD",
                "BACKEND",
                "CAPABILITIES",
                "TRUST",
                "SOURCE",
            ],
            &rows,
        );
        let staged = list
            .as_array()
            .and_then(|a| a.first())
            .and_then(|o| o["official"]["staged"]["build"].as_u64());
        if let Some(b) = staged {
            text.push_str(&format!(
                "
A managed build (b{}) is staged but not activated: activate it in Settings > Runtime.",
                b
            ));
        }
        text
    });
    Ok(())
}

pub struct AddArgs {
    pub github: Option<String>,
    pub local: Option<PathBuf>,
    pub family: Option<String>,
    pub name: Option<String>,
    pub id: Option<String>,
    pub tag: Option<String>,
    pub asset: Option<String>,
    pub backend: Option<String>,
    pub allow_unverified: bool,
    pub copy: bool,
}

fn query_string(pairs: &[(&str, &Option<String>)]) -> String {
    pairs
        .iter()
        .filter_map(|(k, v)| v.as_ref().map(|v| format!("{}={}", k, seg(v))))
        .collect::<Vec<_>>()
        .join("&")
}

/// The runtime family to install: the explicit one, else the one a well-known repository name
/// stands for (`leejet/stable-diffusion.cpp`, `ggml-org/whisper.cpp`, `k2-fsa/sherpa-onnx`); `None` = llama.cpp.
pub fn family_of(explicit: &Option<String>, repo: Option<&str>) -> Option<String> {
    explicit.clone().or_else(|| {
        let repo = repo?.to_ascii_lowercase();
        [
            "stable-diffusion.cpp",
            "whisper.cpp",
            "sherpa-onnx",
            "audio.cpp",
        ]
        .into_iter()
        .find(|f| repo.ends_with(f))
        .map(String::from)
    })
}

pub async fn add(client: &ApiClient, a: AddArgs, out: &Out) -> CliResult<()> {
    let family = family_of(&a.family, a.github.as_deref());
    let mut body = if let Some(repo) = &a.github {
        // Show what will be downloaded (and whether it can be verified) before asking.
        let q = query_string(&[
            ("family", &family),
            ("repo", &Some(repo.clone())),
            ("tag", &a.tag),
            ("asset", &a.asset),
            ("backend", &a.backend),
        ]);
        let pick = client.get(&format!("/runtimes/release?{}", q)).await?;
        let assets = pick["assets"].as_array().cloned().unwrap_or_default();
        let unverified: Vec<String> = pick["unverified"]
            .as_array()
            .map(|u| u.iter().map(str_of).collect())
            .unwrap_or_default();
        for asset in &assets {
            out.info(format!(
                "{}  {}  {}\n  {}",
                str_of(&asset["name"]),
                bytes(asset["size"].as_u64().unwrap_or(0)),
                asset["digest"].as_str().unwrap_or("NO DIGEST"),
                str_of(&asset["download_url"])
            ));
        }
        if !unverified.is_empty() && !a.allow_unverified {
            return Err(CliError::core(format!(
                "runtime.integrity_unverified: {} (use --allow-unverified to install without a digest)",
                unverified.join(", ")
            )));
        }
        let total: u64 = assets.iter().filter_map(|x| x["size"].as_u64()).sum();
        out.confirm(&format!(
            "Download {} from {} ({})?",
            bytes(total),
            repo,
            str_of(&pick["tag"])
        ))?;
        json!({
            "kind": "github",
            "repo": repo,
            "tag": pick["tag"],
            // Only the user's own `--asset`: the tag and backend pick the same archives again,
            // with a CUDA build's runtime companion (`cudart-*`). Sending the first archive's
            // name pinned that one archive and left the CUDA runtime out, so the build fell
            // back to the CPU on a machine without the CUDA toolkit.
            "asset": a.asset,
            "backend": a.backend.clone().map(Value::from).unwrap_or_else(|| pick["backend"].clone()),
            "name": a.name,
            "id": a.id,
            "allow_unverified": a.allow_unverified,
        })
    } else {
        let path = a.local.clone().unwrap_or_default();
        // The API may run in another process: send an absolute path.
        let path = std::fs::canonicalize(&path).unwrap_or(path);
        let path = path
            .to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_string();
        json!({
            "kind": "local",
            "path": path,
            "name": a.name.clone().unwrap_or_default(),
            "id": a.id,
            "copy": a.copy,
        })
    };
    // Absent = llama.cpp (the API's default); a null would be refused.
    if let Some(f) = &family {
        body["family"] = json!(f);
    }
    let request = client.post("/runtimes", &body);
    tokio::pin!(request);
    let entry = loop {
        tokio::select! {
            res = &mut request => break res?,
            _ = tokio::time::sleep(Duration::from_millis(700)) => {
                if a.github.is_some() {
                    if let Ok(p) = client.get("/runtimes/progress").await {
                        if let (Some(done), Some(total)) = (p["downloaded_bytes"].as_u64(), p["total_bytes"].as_u64()) {
                            out.progress(&format!("{} {} / {}", str_of(&p["current_asset"]), bytes(done), bytes(total)));
                        }
                    }
                }
            }
        }
    };
    out.progress_done();
    out.data(&entry, || {
        format!(
            "Added runtime {} ({}), {}, backend {}, trust {}",
            str_of(&entry["id"]),
            str_of(&entry["name"]),
            entry["version"].as_str().unwrap_or("-"),
            entry["backend"].as_str().unwrap_or("-"),
            str_of(&entry["trust"]),
        )
    });
    Ok(())
}

pub async fn rm(client: &ApiClient, id: &str, force: bool, out: &Out) -> CliResult<()> {
    out.confirm(&format!("Remove runtime {}?", id))?;
    let path = if force {
        format!("/runtimes/{}?force=true", seg(id))
    } else {
        format!("/runtimes/{}", seg(id))
    };
    client.delete(&path).await?;
    let value = json!({"removed": id});
    out.data(&value, || format!("Removed runtime {}", id));
    Ok(())
}

pub async fn verify(client: &ApiClient, id: &str, accept: bool, out: &Out) -> CliResult<()> {
    let report = client
        .post(
            &format!("/runtimes/{}/verify", seg(id)),
            &json!({"accept_changes": accept}),
        )
        .await?;
    out.data(&report, || {
        let list = |k: &str| {
            report[k]
                .as_array()
                .map(|a| a.iter().map(str_of).collect::<Vec<_>>().join(", "))
                .filter(|s| !s.is_empty())
        };
        let mut lines = vec![format!(
            "{}: {}",
            id,
            if report["ok"] == true {
                if report["accepted"] == true {
                    "ok (new files accepted)"
                } else {
                    "ok"
                }
            } else {
                "NOT OK"
            }
        )];
        if let Some(v) = report["version"].as_str() {
            lines.push(format!("  {}", v));
        }
        for (k, label) in [
            ("changed", "changed"),
            ("missing", "missing"),
            ("added", "new"),
        ] {
            if let Some(l) = list(k) {
                lines.push(format!("  {}: {}", label, l));
            }
        }
        if let Some(e) = report["error"].as_str() {
            lines.push(format!("  error: {}", crate::messages::core_text(e)));
        }
        lines.join("\n")
    });
    if report["ok"] != true {
        return Err(CliError::core(format!("runtime.changed: {}", id)));
    }
    Ok(())
}

pub async fn suggest(client: &ApiClient, model: &str, out: &Out) -> CliResult<()> {
    let advice = client
        .get(&format!("/runtimes/advice?model={}", seg(model)))
        .await?;
    out.data(&advice, || {
        let types: Vec<String> = advice["special_types"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|t| match t["name"].as_str() {
                        Some(n) => format!("{} ({})", n, t["id"]),
                        None => format!("#{}", t["id"]),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if types.is_empty() {
            return "Standard tensor types: every runtime can load this model.".into();
        }
        let mut lines = vec![format!("Tensor types to check: {}", types.join(", "))];
        for r in advice["runtimes"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or_default()
        {
            lines.push(format!(
                "  {:<20} {}",
                str_of(&r["runtime_id"]),
                str_of(&r["support"])
            ));
        }
        if let Some(id) = advice["suggested_runtime"].as_str() {
            lines.push(format!("Suggested runtime: {}", id));
        } else if let Some(f) = advice["fork"].as_object() {
            lines.push(format!(
                "The official runtime can't load it. Add {}: harness runtime add --github {}",
                f.get("name").map(str_of).unwrap_or_default(),
                f.get("repo").map(str_of).unwrap_or_default()
            ));
        } else if advice["official_unsupported"] == true {
            lines.push("The official runtime can't load it and no registered runtime can.".into());
        }
        lines.join("\n")
    });
    Ok(())
}

fn managed_text(v: &Value) -> String {
    let opt = |x: &Value| x.as_str().unwrap_or("-").to_string();
    let backup = match v["backup"].as_object() {
        Some(b) => format!(
            "{} ({})",
            b.get("version")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown build"),
            b.get("backend").and_then(|x| x.as_str()).unwrap_or("-")
        ),
        None => "none".into(),
    };
    format!(
        "Installed:  {} ({} recorded, {} detected)\nPrevious:   {}\nBackup:     {}\nAuto-update: {}",
        opt(&v["installed_version"]),
        opt(&v["backend"]),
        opt(&v["installed_backend"]),
        opt(&v["previous_version"]),
        backup,
        if v["auto_update"] == true { "on" } else { "off" }
    )
}

/// `runtime managed status`.
pub async fn managed_status(client: &ApiClient, out: &Out) -> CliResult<()> {
    let v = client.get("/runtime/managed").await?;
    out.data(&v, || managed_text(&v));
    Ok(())
}

/// `runtime managed install [--backend B]`: the same steps as Settings > Runtime.
pub async fn managed_install(
    client: &ApiClient,
    backend: Option<String>,
    allow_unverified: bool,
    out: &Out,
) -> CliResult<()> {
    out.confirm(&format!(
        "Download and activate the newest official llama.cpp build ({})?",
        backend.as_deref().unwrap_or("installed backend")
    ))?;
    let body = json!({ "backend": backend, "allow_unverified": allow_unverified });
    let request = client.post("/runtime/managed/install", &body);
    tokio::pin!(request);
    let v = loop {
        tokio::select! {
            res = &mut request => break res?,
            _ = tokio::time::sleep(Duration::from_millis(700)) => {
                if let Ok(status) = client.get("/runtime/managed").await {
                    let p = &status["download"];
                    if let (Some(done), Some(total)) =
                        (p["downloaded_bytes"].as_u64(), p["total_bytes"].as_u64())
                    {
                        out.info(format!("{}: {} / {}", str_of(&p["current_asset"]), bytes(done), bytes(total)));
                    }
                }
            }
        }
    };
    out.data(&v, || {
        format!(
            "Installed {} ({}), previous: {}\n{}",
            str_of(&v["version"]),
            v["backend"].as_str().unwrap_or("-"),
            v["previous_version"].as_str().unwrap_or("-"),
            str_of(&v["version_text"])
        )
    });
    Ok(())
}

/// `runtime managed rollback`.
pub async fn managed_rollback(client: &ApiClient, out: &Out) -> CliResult<()> {
    out.confirm("Restore the build in the backup slot?")?;
    let v = client.post("/runtime/managed/rollback", &json!({})).await?;
    out.data(&v, || managed_text(&v));
    Ok(())
}

#[cfg(test)]
mod family_tests {
    use super::family_of;

    #[test]
    fn the_family_follows_a_well_known_repository() {
        let none = None;
        assert_eq!(
            family_of(&none, Some("leejet/stable-diffusion.cpp")).as_deref(),
            Some("stable-diffusion.cpp")
        );
        assert_eq!(
            family_of(&none, Some("ggml-org/whisper.cpp")).as_deref(),
            Some("whisper.cpp")
        );
        assert_eq!(
            family_of(&none, Some("0xShug0/audio.cpp")).as_deref(),
            Some("audio.cpp")
        );
        assert_eq!(family_of(&none, Some("PrismML-Eng/llama.cpp")), None);
        assert_eq!(family_of(&none, None), None);
        // An explicit family wins.
        assert_eq!(
            family_of(
                &Some("llama.cpp".into()),
                Some("leejet/stable-diffusion.cpp")
            )
            .as_deref(),
            Some("llama.cpp")
        );
    }
}
