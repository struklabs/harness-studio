//! Official catalog snapshots. Metadata and artifacts always share one immutable revision.
use super::package::{self, Permissions};
use crate::store::JsonStore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::Path, time::Duration};

const RAW: &str = "https://raw.githubusercontent.com/struklabs/harness-registry";
const HEAD: &str = "https://api.github.com/repos/struklabs/harness-registry/commits/main";
const LIMIT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Description {
    pub en: String,
    pub tr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Tool,
    Skill,
    Mcp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub publisher: String,
    pub name: String,
    pub description: Description,
    pub version: String,
    pub license: String,
    pub homepage: String,
    pub source: String,
    pub platforms: Vec<String>,
    pub permissions: Permissions,
    pub tags: Vec<String>,
    pub min_harness_version: String,
    pub artifact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub schema_version: u32,
    pub entries: Vec<Entry>,
}

fn https(s: &str) -> bool {
    reqwest::Url::parse(s).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}

pub fn parse(text: &str) -> Result<Index, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if let Some(entries) = value["entries"].as_array() {
        for e in entries {
            for field in [
                "/permissions/network",
                "/permissions/fs/read",
                "/permissions/fs/write",
                "/permissions/shell",
            ] {
                if e.pointer(field).is_none() {
                    return Err("Missing registry permissions".into());
                }
            }
        }
    }
    let index: Index = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if index.schema_version != 1 {
        return Err("Unsupported registry schema".into());
    }
    let mut ids = BTreeSet::new();
    for e in &index.entries {
        let (kind, root, suffix) = match e.kind {
            Kind::Tool => ("tool", "tools", ""),
            Kind::Skill => ("skill", "skills", "/SKILL.md"),
            Kind::Mcp => ("mcp", "mcp", "/config.json"),
        };
        let dir =
            e.id.strip_prefix(&format!("struklabs.{kind}."))
                .unwrap_or("");
        let valid_dir = dir.starts_with(|c: char| c.is_ascii_lowercase())
            && dir
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
        if !valid_dir
            || e.artifact != format!("{root}/{dir}{suffix}")
            || !ids.insert(&e.id)
            || e.publisher != "StrukLabs"
            || e.name.trim().is_empty()
            || e.license.trim().is_empty()
            || [&e.description.en, &e.description.tr]
                .iter()
                .any(|s| s.trim().is_empty() || s.chars().count() > 1024)
            || !https(&e.homepage)
            || !https(&e.source)
            || e.platforms.is_empty()
            || e.platforms
                .iter()
                .any(|p| !["windows", "macos", "linux"].contains(&p.as_str()))
            || e.platforms.iter().collect::<BTreeSet<_>>().len() != e.platforms.len()
            || e.permissions.shell
            || e.permissions.network.iter().any(|s| s == "*")
            || e.tags.is_empty()
            || e.tags.iter().collect::<BTreeSet<_>>().len() != e.tags.len()
            || e.tags.iter().any(|t| {
                t.split('-').any(|part| {
                    part.is_empty()
                        || !part
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                })
            })
        {
            return Err(format!("Invalid registry entry: {}", e.id));
        }
        for v in [&e.version, &e.min_harness_version] {
            if v.len() > 64 || semver::Version::parse(v).is_err() {
                return Err("Invalid registry version".into());
            }
        }
        package::check_permissions(&e.permissions)?;
    }
    Ok(index)
}

impl Index {
    pub fn compatible(&self, platform: &str, version: &str) -> Vec<Entry> {
        let Ok(version) = semver::Version::parse(version) else {
            return vec![];
        };
        self.entries
            .iter()
            .filter(|e| {
                e.platforms.iter().any(|p| p == platform)
                    && semver::Version::parse(&e.min_harness_version)
                        .is_ok_and(|min| version.cmp_precedence(&min).is_ge())
            })
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub commit: String,
    pub etag: Option<String>,
    pub index: Index,
    pub offline: bool,
}

fn commit_valid(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Harness-Studio")
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())
}

async fn body(mut response: reqwest::Response, limit: usize) -> Result<String, String> {
    response.error_for_status_ref().map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > limit {
            return Err("Registry file too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn cached(path: &Path) -> Option<Snapshot> {
    let mut s: Snapshot = JsonStore::new(path).load().ok()?;
    if !commit_valid(&s.commit) {
        return None;
    }
    s.index = parse(&serde_json::to_string(&s.index).ok()?).ok()?;
    Some(s)
}

async fn refresh(
    client: &reqwest::Client,
    head: &str,
    raw: &str,
    old: Option<&Snapshot>,
) -> Result<Snapshot, String> {
    let head = body(
        client.get(head).send().await.map_err(|e| e.to_string())?,
        LIMIT,
    )
    .await?;
    let commit = serde_json::from_str::<serde_json::Value>(&head).map_err(|e| e.to_string())?
        ["sha"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if !commit_valid(&commit) {
        return Err("Invalid registry revision".into());
    }
    let mut request = client.get(format!("{raw}/{commit}/index.json"));
    if let Some(etag) = old
        .filter(|s| s.commit == commit)
        .and_then(|s| s.etag.as_ref())
    {
        request = request.header(reqwest::header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        let mut s = old
            .filter(|s| s.commit == commit)
            .ok_or("Registry returned 304 without cache")?
            .clone();
        s.offline = false;
        return Ok(s);
    }
    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    Ok(Snapshot {
        commit,
        etag,
        index: parse(&body(response, LIMIT).await?)?,
        offline: false,
    })
}

async fn fetch_from(
    path: &Path,
    client: &reqwest::Client,
    head: &str,
    raw: &str,
) -> Result<Snapshot, String> {
    let old = cached(path);
    match refresh(client, head, raw, old.as_ref()).await {
        Ok(s) => {
            JsonStore::new(path).save(&s)?;
            Ok(s)
        }
        Err(e) => old
            .map(|mut s| {
                s.offline = true;
                s
            })
            .ok_or(e),
    }
}

pub async fn fetch() -> Result<Snapshot, String> {
    fetch_from(
        &crate::paths::data_root().join("registry/index.json"),
        &client()?,
        HEAD,
        RAW,
    )
    .await
}

pub fn selected(commit: &str, id: &str) -> Result<(Snapshot, Entry), String> {
    let snapshot = cached(&crate::paths::data_root().join("registry/index.json"))
        .ok_or("Registry cache unavailable")?;
    if snapshot.commit != commit {
        return Err("Registry changed; refresh before installing".into());
    }
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else {
        std::env::consts::OS
    };
    let entry = snapshot
        .index
        .compatible(platform, env!("CARGO_PKG_VERSION"))
        .into_iter()
        .find(|e| e.id == id)
        .ok_or("Registry entry unavailable")?;
    Ok((snapshot, entry))
}

/// Returns a validated staging folder; the normal install/approval path owns installation.
pub async fn prepare_tool(snapshot: &Snapshot, entry: &Entry) -> Result<tempfile::TempDir, String> {
    prepare_tool_from(snapshot, entry, RAW).await
}

async fn prepare_tool_from(
    snapshot: &Snapshot,
    entry: &Entry,
    raw: &str,
) -> Result<tempfile::TempDir, String> {
    let client = client()?;
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let base = format!("{raw}/{}/{}/", snapshot.commit, entry.artifact);
    let text = body(
        client
            .get(format!("{base}manifest.json"))
            .send()
            .await
            .map_err(|e| e.to_string())?,
        package::MAX_FILE_BYTES as usize,
    )
    .await?;
    let manifest = package::parse_manifest(&text)?;
    if manifest.name != entry.id.trim_start_matches("struklabs.tool.")
        || manifest.version != entry.version
        || manifest.permissions != entry.permissions
    {
        return Err("Registry manifest does not match declared metadata".into());
    }
    std::fs::write(dir.path().join("manifest.json"), text).map_err(|e| e.to_string())?;
    for file in std::iter::once(&manifest.entry).chain(manifest.test.as_ref()) {
        let mut url = reqwest::Url::parse(&base).map_err(|e| e.to_string())?;
        url.path_segments_mut()
            .map_err(|_| "Invalid registry URL")?
            .pop_if_empty()
            .extend(file.split('/'));
        let text = body(
            client.get(url).send().await.map_err(|e| e.to_string())?,
            package::MAX_FILE_BYTES as usize,
        )
        .await?;
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().ok_or("Invalid package path")?)
            .map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())?;
    }
    package::load_dir(dir.path())?;
    Ok(dir)
}

pub async fn prepare_mcp(
    snapshot: &Snapshot,
    entry: &Entry,
) -> Result<crate::mcp::McpConfig, String> {
    let text = body(
        client()?
            .get(format!("{RAW}/{}/{}", snapshot.commit, entry.artifact))
            .send()
            .await
            .map_err(|e| e.to_string())?,
        package::MAX_FILE_BYTES as usize,
    )
    .await?;
    parse_mcp(&text, entry)
}

fn parse_mcp(text: &str, entry: &Entry) -> Result<crate::mcp::McpConfig, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Config {
        #[serde(rename = "mcpServers")]
        servers: std::collections::BTreeMap<String, Server>,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Server {
        Http(Http),
        Stdio(Stdio),
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Http {
        url: String,
        transport: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Stdio {
        command: String,
        args: Vec<String>,
    }
    let strict: Config = serde_json::from_str(text).map_err(|e| e.to_string())?;
    for server in strict.servers.values() {
        match server {
            Server::Http(s) => {
                let url = reqwest::Url::parse(&s.url).map_err(|e| e.to_string())?;
                if !https(&s.url)
                    || !crate::mcp::TRANSPORTS.contains(&s.transport.as_str())
                    || !url
                        .host_str()
                        .is_some_and(|h| package::domain_allowed(&entry.permissions.network, h))
                {
                    return Err("MCP endpoint does not match declared permissions".into());
                }
            }
            Server::Stdio(s) => {
                if s.command.trim().is_empty() || s.args.iter().any(|a| a.contains('\0')) {
                    return Err("Invalid MCP command".into());
                }
            }
        }
    }
    let config: crate::mcp::McpConfig = serde_json::from_str(text).map_err(|e| e.to_string())?;
    crate::mcp::validate(&config)?;
    if config.mcp_servers.len() != 1
        || config
            .mcp_servers
            .values()
            .any(|s| !s.env.is_empty() || !s.headers.is_empty())
    {
        return Err("Invalid official MCP configuration".into());
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const FIXTURE: &str = include_str!("fixtures/registry-index.json");
    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn parses_all_schema_fields_and_filters_platform_and_semver() {
        let index = parse(FIXTURE).unwrap();
        assert_eq!(index.entries.len(), 3);
        assert_eq!(
            index.entries[0].permissions.network,
            ["learn.microsoft.com"]
        );
        assert_eq!(index.entries[2].artifact, "tools/json_format");
        assert!(!index.entries[2].description.tr.is_empty());
        assert_eq!(index.compatible("windows", "0.1.2").len(), 3);
        assert!(index.compatible("android", "0.1.2").is_empty());
        assert!(index.compatible("linux", "0.1.0-alpha.1").is_empty());
        let mut index = index;
        index.entries[0].min_harness_version = "0.10.0".into();
        index.entries[1].platforms = vec!["macos".into()];
        assert_eq!(index.compatible("linux", "0.2.0").len(), 1);
        assert_eq!(index.compatible("macos", "0.10.0").len(), 3);
        index.entries[0].min_harness_version = "0.1.2+registry.1".into();
        assert_eq!(index.compatible("linux", "0.1.2").len(), 2);
    }

    #[test]
    fn rejects_invalid_metadata_permissions_and_schema() {
        for (pointer, value) in [
            ("/schema_version", json!(2)),
            ("/entries/0/type", json!("community")),
            ("/entries/0/publisher", json!("Other")),
            ("/entries/0/artifact", json!("../config.json")),
            ("/entries/0/version", json!("01.0.0")),
            ("/entries/0/min_harness_version", json!("1.0")),
            ("/entries/0/source", json!("javascript:alert(1)")),
            ("/entries/0/platforms", json!(["windows", "windows"])),
            ("/entries/0/permissions/shell", json!(true)),
            (
                "/entries/0/permissions/network",
                json!(["https://example.com"]),
            ),
            ("/entries/0/permissions/fs/read", json!(["../private"])),
            ("/entries/0/description/tr", json!(" ")),
        ] {
            let mut index: Value = serde_json::from_str(FIXTURE).unwrap();
            *index.pointer_mut(pointer).unwrap() = value;
            assert!(parse(&index.to_string()).is_err(), "{pointer}");
        }
        let mut index: Value = serde_json::from_str(FIXTURE).unwrap();
        index["entries"][0]["permissions"]
            .as_object_mut()
            .unwrap()
            .remove("network");
        assert!(parse(&index.to_string()).is_err());
        let mut index: Value = serde_json::from_str(FIXTURE).unwrap();
        index["entries"][1] = index["entries"][0].clone();
        assert!(parse(&index.to_string()).is_err());
        assert!(parse("{}").is_err());
    }

    // Real HTTP requests, with scripted responses and captured conditional headers.
    async fn server(
        responses: Vec<(u16, String)>,
    ) -> (String, tokio::task::JoinHandle<Vec<String>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut requests = vec![];
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = vec![];
                loop {
                    let mut buf = [0; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    bytes.extend_from_slice(&buf[..n]);
                    if n == 0 || bytes.ends_with(b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8(bytes).unwrap());
                let response = format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nETag: \"catalog-v1\"\r\nConnection: close\r\n\r\n{body}", body.len());
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            requests
        });
        (base, task)
    }

    #[tokio::test]
    async fn cache_revalidates_etag_and_recovers_offline_without_losing_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache/index.json");
        let head = json!({"sha": SHA}).to_string();
        let (base, task) = server(vec![
            (200, head.clone()),
            (200, FIXTURE.into()),
            (200, head.clone()),
            (304, String::new()),
            (200, head),
            (200, "broken index".into()),
        ])
        .await;
        let client = client().unwrap();
        let first = fetch_from(&path, &client, &format!("{base}/head"), &base)
            .await
            .unwrap();
        assert!(!first.offline);
        assert_eq!(first.commit, SHA);
        let second = fetch_from(&path, &client, &format!("{base}/head"), &base)
            .await
            .unwrap();
        assert!(!second.offline);
        let fallback = fetch_from(&path, &client, &format!("{base}/head"), &base)
            .await
            .unwrap();
        assert!(fallback.offline);
        let requests = task.await.unwrap();
        assert!(requests[1].starts_with(&format!("GET /{SHA}/index.json ")));
        assert!(!requests[1].to_ascii_lowercase().contains("if-none-match"));
        assert!(requests[3]
            .to_ascii_lowercase()
            .contains("if-none-match: \"catalog-v1\""));
        let offline = fetch_from(&path, &client, &format!("{base}/head"), &base)
            .await
            .unwrap();
        assert!(offline.offline);
        assert_eq!(offline.index.entries.len(), 3);
        assert!(
            fetch_from(&temp.path().join("missing"), &client, &base, &base)
                .await
                .is_err()
        );
        std::fs::write(&path, "broken cache").unwrap();
        assert!(cached(&path).is_none());
    }

    #[tokio::test]
    async fn timeout_uses_cache_and_changed_revision_does_not_reuse_etag() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("index.json");
        let old = Snapshot {
            commit: SHA.into(),
            etag: Some("old".into()),
            index: parse(FIXTURE).unwrap(),
            offline: false,
        };
        JsonStore::new(&path).save(&old).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let slow = reqwest::Client::builder()
            .timeout(Duration::from_millis(50))
            .build()
            .unwrap();
        assert!(fetch_from(&path, &slow, &url, &url).await.unwrap().offline);
        let new_sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let (base, task) = server(vec![
            (200, json!({"sha": new_sha}).to_string()),
            (200, FIXTURE.into()),
        ])
        .await;
        assert_eq!(
            fetch_from(&path, &client().unwrap(), &base, &base)
                .await
                .unwrap()
                .commit,
            new_sha
        );
        assert!(!task.await.unwrap()[1]
            .to_ascii_lowercase()
            .contains("if-none-match"));
    }

    #[test]
    fn mcp_subset_checks_endpoint_permissions_and_rejects_credentials() {
        let entry = &parse(FIXTURE).unwrap().entries[0];
        let good = json!({"mcpServers": {"microsoft-learn": {"url": "https://learn.microsoft.com/api/mcp", "transport": "http"}}});
        assert!(parse_mcp(&good.to_string(), entry).is_ok());
        let mut bad = good.clone();
        bad["mcpServers"]["microsoft-learn"]["url"] = json!("https://other.example/mcp");
        assert!(parse_mcp(&bad.to_string(), entry).is_err());
        let mut bad = good;
        bad["mcpServers"]["microsoft-learn"]["headers"] = json!({"Authorization": "secret"});
        assert!(parse_mcp(&bad.to_string(), entry).is_err());
    }

    #[tokio::test]
    async fn tool_download_uses_revision_and_existing_package_installer() {
        let entry = &parse(FIXTURE).unwrap().entries[2];
        let snapshot = Snapshot {
            commit: SHA.into(),
            etag: None,
            index: parse(FIXTURE).unwrap(),
            offline: false,
        };
        let manifest = json!({"name":"json_format", "version":"1.0.0", "description":"Format JSON", "input_schema":{"type":"object"}, "output":"JSON", "runtime":"js", "entry":"index.js", "permissions":entry.permissions});
        let (base, task) = server(vec![
            (200, manifest.to_string()),
            (200, "function run(args) { return 'ok'; }".into()),
        ])
        .await;
        let staged = prepare_tool_from(&snapshot, entry, &base).await.unwrap();
        let dest = tempfile::tempdir().unwrap();
        let registry = super::super::ToolRegistry::new();
        let installed =
            super::super::installed::install(dest.path(), staged.path(), &registry).unwrap();
        assert_eq!(installed.name, "json_format");
        assert!(task
            .await
            .unwrap()
            .iter()
            .all(|r| r.starts_with(&format!("GET /{SHA}/tools/json_format/"))));
        super::super::installed::remove(dest.path(), "json_format").unwrap();
        assert!(super::super::installed::list(dest.path()).is_empty());
        let mut bad = manifest;
        bad["permissions"]["network"] = json!(["example.com"]);
        let (base, task) = server(vec![(200, bad.to_string())]).await;
        assert!(prepare_tool_from(&snapshot, entry, &base).await.is_err());
        task.await.unwrap();
    }
}
