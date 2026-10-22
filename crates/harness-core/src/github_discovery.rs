//! Public repository discovery. Never executes repository content or installs MCP servers.
#[cfg(test)]
use crate::secrets::SecretStore;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const LIMIT: usize = 1024 * 1024;
const TTL: u64 = 900;
const ACCOUNT: &str = "github-discovery-token";

pub fn token_set(token: &str) -> Result<(), String> {
    if token.trim().is_empty() {
        crate::secrets::store().delete(ACCOUNT)
    } else {
        crate::secrets::store().set(ACCOUNT, token.trim())
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn repository(input: &str) -> Result<String, String> {
    let input = input.trim();
    let path = if input.starts_with("https://") {
        let u = reqwest::Url::parse(input).map_err(|_| "Invalid repository URL")?;
        if u.host_str() != Some("github.com")
            || !u.username().is_empty()
            || u.password().is_some()
            || u.port().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err("Use a public github.com repository URL".into());
        }
        u.path().trim_matches('/').to_string()
    } else {
        input.to_string()
    };
    let path = path.strip_suffix(".git").unwrap_or(&path);
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|s| {
            s.is_empty()
                || *s == "."
                || *s == ".."
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
    {
        return Err("Use owner/repository or a repository URL".into());
    }
    Ok(path.into())
}

#[derive(Serialize, Deserialize)]
struct Cache {
    expires: u64,
    value: Value,
}

struct Github {
    client: reqwest::Client,
    base: String,
    /// Origin of file contents at a commit (`raw.githubusercontent.com`).
    raw: String,
    token: Option<String>,
    cache: std::path::PathBuf,
}
impl Github {
    fn new() -> Result<Self, String> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(25))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("Harness-Studio")
                .build()
                .map_err(|e| e.to_string())?,
            base: "https://api.github.com".into(),
            raw: "https://raw.githubusercontent.com".into(),
            token: crate::secrets::store().get(ACCOUNT).unwrap_or(None),
            cache: crate::paths::data_root().join("cache/github-discovery"),
        })
    }
    async fn get(&self, path: &str) -> Result<Value, String> {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(format!("{}{path}", self.base).as_bytes());
        let key = format!(
            "{}.json",
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let store = crate::store::JsonStore::new(self.cache.join(key));
        if let Ok(c) = store.load::<Cache>() {
            if c.expires > now() {
                return Ok(c.value);
            }
        }
        // Persist backoff across queries and app restarts, without storing credentials.
        let backoff = crate::store::JsonStore::new(self.cache.join("backoff.json"));
        let until = backoff.load::<u64>().unwrap_or(0);
        if until > now() {
            return Err(format!(
                "GitHub rate limit reached; retry after the current cooldown ({} seconds) or add a GitHub token",
                until - now()
            ));
        }
        let mut request = self
            .client
            .get(format!("{}{path}", self.base))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let mut response = request.send().await.map_err(|_| "GitHub request failed")?;
        if response.status() == 429 || response.status() == 403 {
            let headers = response.headers();
            let until = headers
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(|s| now().saturating_add(s))
                .or_else(|| {
                    headers
                        .get("x-ratelimit-reset")
                        .and_then(|h| h.to_str().ok())
                        .and_then(|s| s.parse().ok())
                })
                .unwrap_or(now() + 60)
                .max(now() + 1);
            backoff.save(&until)?;
            return Err(format!(
                "GitHub rate limit or access restriction; retry after {} seconds",
                until.saturating_sub(now())
            ));
        }
        if !response.status().is_success() {
            return Err(format!(
                "GitHub returned HTTP {}",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "GitHub response failed")?
        {
            if bytes.len() + chunk.len() > 8 * LIMIT {
                return Err("GitHub response exceeds size limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid GitHub response")?;
        store.save(&Cache {
            expires: now() + TTL,
            value: value.clone(),
        })?;
        Ok(value)
    }
    async fn blob(&self, repo: &str, sha: &str) -> Result<Vec<u8>, String> {
        let v = self.get(&format!("/repos/{repo}/git/blobs/{sha}")).await?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(
                v["content"]
                    .as_str()
                    .ok_or("Missing blob content")?
                    .replace('\n', ""),
            )
            .map_err(|_| "Invalid blob encoding")?;
        if bytes.len() > LIMIT {
            return Err("File exceeds 1 MiB".into());
        }
        Ok(bytes)
    }
    /// A file of the tree at `commit`. The commit-pinned raw address does not count against the
    /// API rate limit (60 requests an hour without a token, which one review of a skills
    /// repository used up); the API blob is the fallback.
    async fn file(&self, repo: &str, commit: &str, entry: &Value) -> Result<Vec<u8>, String> {
        let sha = entry["sha"].as_str().ok_or("Missing blob SHA")?;
        if let Some(bytes) = self.raw_file(repo, commit, entry).await {
            return Ok(bytes);
        }
        self.blob(repo, sha).await
    }
    async fn raw_file(&self, repo: &str, commit: &str, entry: &Value) -> Option<Vec<u8>> {
        let path = entry["path"].as_str()?;
        let size = entry["size"].as_u64()?;
        if size > LIMIT as u64 {
            return None;
        }
        let mut url = reqwest::Url::parse(&self.raw).ok()?;
        url.path_segments_mut()
            .ok()?
            .extend(repo.split('/'))
            .push(commit)
            .extend(path.split('/'));
        let response = self.client.get(url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let bytes = response.bytes().await.ok()?;
        // The tree names the size; anything else is not the file of this commit.
        (bytes.len() as u64 == size).then(|| bytes.to_vec())
    }
    async fn tree(&self, repo: &str, sha: &str) -> Result<Vec<Value>, String> {
        let v = self
            .get(&format!("/repos/{repo}/git/trees/{sha}?recursive=1"))
            .await?;
        if v["truncated"] == true {
            return Err("Repository tree is incomplete; review a smaller repository".into());
        }
        Ok(v["tree"]
            .as_array()
            .ok_or("Missing repository tree")?
            .clone())
    }
}

fn public(v: &Value) -> bool {
    v["private"] == false
        && v["full_name"]
            .as_str()
            .is_some_and(|s| repository(s).is_ok())
}
pub fn parse_search(v: &Value) -> Result<Vec<Value>, String> {
    Ok(v["items"]
        .as_array()
        .ok_or("Invalid GitHub search response")?
        .iter()
        .filter(|v| public(v))
        .cloned()
        .collect())
}
pub async fn search(query: &str) -> Result<Vec<Value>, String> {
    let gh = Github::new()?;
    if query.trim().is_empty() || query.len() > 256 {
        return Err("Enter a search query or repository URL (up to 256 characters)".into());
    }
    if query.trim().starts_with("https://") || repository(query).is_ok() {
        let repo = repository(query)?;
        let v = gh.get(&format!("/repos/{repo}")).await?;
        if !public(&v) {
            return Err("Only public repositories are supported".into());
        }
        return Ok(vec![v]);
    }
    let q: String = reqwest::Url::parse_with_params(
        "https://api.github.com/search/repositories",
        &[
            ("q", format!("{} is:public", query.trim())),
            ("per_page", "20".into()),
        ],
    )
    .map_err(|e| e.to_string())?
    .query()
    .unwrap_or_default()
    .into();
    parse_search(&gh.get(&format!("/search/repositories?{q}")).await?)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SkillReview {
    pub path: String,
    pub name: String,
    pub errors: Vec<String>,
    pub scripts: Vec<String>,
    pub network: Vec<String>,
    pub bytes: usize,
    /// Files and directories SKILL.md points to that exist in the skill.
    #[serde(default)]
    pub references: Vec<String>,
    /// Problems that do not block installation (e.g. a script SKILL.md never mentions).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Risky patterns found in the skill's scripts (static text scan; nothing is executed).
    #[serde(default)]
    pub risks: Vec<RiskFinding>,
    #[serde(default)]
    pub security: SecuritySummary,
    /// Commit the review and the install are pinned to.
    #[serde(default)]
    pub commit: String,
    /// Where an install lands: always the user scope, never a project or workspace.
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub install_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskFinding {
    pub file: String,
    /// `network`, `shell`, `eval`, `credentials` or `binary`.
    pub category: String,
    /// The matched pattern, shown to the user.
    pub pattern: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecuritySummary {
    /// `none` (no scripts), `low`, `medium` or `high`.
    pub level: String,
    pub scripts_scanned: usize,
    pub network: usize,
    pub shell: usize,
    pub eval: usize,
    pub credentials: usize,
    pub binary: usize,
}

type RiskRule = (&'static str, &'static str, &'static str);
/// (category, label, case-insensitive regex). Plain text matching: a hit is a reason to read
/// the script, not proof of harm, and a miss is not proof of safety.
const RISK_RULES: &[RiskRule] = &[
    ("network", "http(s) URL", r"https?://"),
    ("network", "curl / wget", r"\b(?:curl|wget)\b"),
    (
        "network",
        "Invoke-WebRequest",
        r"invoke-(?:webrequest|restmethod)|\b(?:iwr|irm)\b",
    ),
    (
        "network",
        "fetch / requests / urllib / httpx / axios",
        r"\bfetch\(|\brequests\.|\burllib|\bhttpx\b|\baxios\b",
    ),
    ("network", "socket", r"\bsocket\b|\bnc\s+-"),
    (
        "shell",
        "subprocess / os.system / Popen",
        r"\bsubprocess\b|\bos\.system\b|\bpopen\b",
    ),
    (
        "shell",
        "child_process / exec",
        r"\bchild_process\b|\bexecsync\b|\bspawnsync\b",
    ),
    (
        "shell",
        "sh -c / bash -c / cmd /c",
        r"\b(?:ba|z)?sh\s+-c\b|\bcmd(?:\.exe)?\s+/c\b|\bpowershell(?:\.exe)?\b|\bpwsh\b",
    ),
    ("shell", "Start-Process", r"start-process"),
    (
        "shell",
        "destructive delete",
        r"\brm\s+-rf?\b|\bremove-item\b.*-recurse|\bdel\s+/[sq]\b",
    ),
    (
        "eval",
        "eval / exec of dynamic code",
        r"\beval\s*\(|\bexec\s*\(|new\s+function\s*\(|\binvoke-expression\b|\biex\b",
    ),
    (
        "eval",
        "decode and run",
        r"base64\s+(?:-d|--decode)|frombase64string|\batob\s*\(|\|\s*(?:ba)?sh\b",
    ),
    (
        "credentials",
        "credential files and stores",
        r"\.ssh\b|\.aws\b|\.npmrc|\.netrc|\bid_rsa|\.kube[/\\]config|\.gnupg|login data|\bcredentials(?:\.json)?\b|\.git-credentials",
    ),
    (
        "credentials",
        "environment secrets",
        r"\.env\b|(?:api[_-]?key|secret|token|password)s?\s*[=:]|os\.environ|process\.env|getenv\(",
    ),
    (
        "credentials",
        "keychains and browser data",
        r"\bkeychain\b|\bcookies\b|appdata[/\\]roaming|\bsecurity\s+find-",
    ),
];

fn risk_rules() -> &'static Vec<(&'static str, &'static str, regex::Regex)> {
    static RULES: std::sync::OnceLock<Vec<(&'static str, &'static str, regex::Regex)>> =
        std::sync::OnceLock::new();
    RULES.get_or_init(|| {
        RISK_RULES
            .iter()
            .map(|(c, l, p)| {
                (
                    *c,
                    *l,
                    regex::RegexBuilder::new(p)
                        .case_insensitive(true)
                        .build()
                        .expect("constant regex"),
                )
            })
            .collect()
    })
}

const BINARY_EXTENSIONS: &[&str] = &["exe", "dll", "so", "dylib"];

/// Scans one script's text; at most one finding per rule and line, 50 per file.
pub fn scan_script(path: &str, bytes: &[u8]) -> Vec<RiskFinding> {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if BINARY_EXTENSIONS.contains(&ext.as_str()) || bytes.contains(&0) {
        return vec![RiskFinding {
            file: path.into(),
            category: "binary".into(),
            pattern: "compiled code that cannot be read".into(),
            line: 0,
        }];
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = vec![];
    for (i, line) in text.lines().enumerate() {
        for (category, label, re) in risk_rules() {
            if out.len() >= 50 {
                return out;
            }
            if re.is_match(line) {
                out.push(RiskFinding {
                    file: path.into(),
                    category: (*category).into(),
                    pattern: (*label).into(),
                    line: i + 1,
                });
            }
        }
    }
    out
}

pub fn summarize(scripts: &[String], risks: &[RiskFinding]) -> SecuritySummary {
    let count = |c: &str| risks.iter().filter(|r| r.category == c).count();
    let mut s = SecuritySummary {
        level: String::new(),
        scripts_scanned: scripts.len(),
        network: count("network"),
        shell: count("shell"),
        eval: count("eval"),
        credentials: count("credentials"),
        binary: count("binary"),
    };
    s.level = if scripts.is_empty() {
        "none"
    } else if s.credentials > 0 || s.binary > 0 || (s.eval > 0 && s.network > 0) {
        "high"
    } else if s.shell > 0 || s.eval > 0 || s.network > 0 {
        "medium"
    } else {
        "low"
    }
    .into();
    s
}

fn safe(path: &str) -> bool {
    !path.is_empty()
        && !path
            .chars()
            .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
        && path.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && !p.ends_with(['.', ' '])
                && !matches!(
                    p.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
}
pub fn validate_skill(path: &str, files: &BTreeMap<String, Vec<u8>>) -> SkillReview {
    let mut r = SkillReview {
        path: path.into(),
        name: String::new(),
        errors: vec![],
        scripts: vec![],
        network: vec![],
        bytes: files.values().map(Vec::len).sum(),
        references: vec![],
        warnings: vec![],
        risks: vec![],
        security: SecuritySummary::default(),
        commit: String::new(),
        scope: "user".into(),
        install_dir: String::new(),
    };
    let text = files
        .get("SKILL.md")
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("");
    match crate::skills::parse(text) {
        Ok((n, _, _)) => r.name = n,
        Err(e) => r.errors.push(e),
    }
    if r.bytes > 4 * LIMIT || files.len() > 128 || files.values().any(|b| b.len() > LIMIT) {
        r.errors
            .push("Skill exceeds 128 files, 1 MiB per file or 4 MiB total".into());
    }
    let refs =
        regex::Regex::new(r#"(?:^|[\s`('"=])((?:scripts|references|assets)/[a-zA-Z0-9_./-]*)"#)
            .expect("constant regex");
    let exists = |p: &str| {
        if p.ends_with('/') {
            safe(p.trim_end_matches('/')) && files.keys().any(|f| f.starts_with(p))
        } else {
            safe(p) && files.contains_key(p)
        }
    };
    for reference in refs.captures_iter(text) {
        let p = reference[1].trim_end_matches('.');
        if !exists(p) {
            r.errors.push(format!("Missing or unsafe resource: {p}"));
        } else if !r.references.iter().any(|x| x == p) {
            r.references.push(p.to_string());
        }
    }
    let links =
        regex::Regex::new(r#"\]\(<?([^\s)>]+)>?(?:\s+"[^"]*")?\)"#).expect("constant regex");
    let definitions =
        regex::Regex::new(r"(?m)^\s*\[[^\]]+\]:\s*<?([^\s>]+)>?").expect("constant regex");
    for link in links
        .captures_iter(text)
        .chain(definitions.captures_iter(text))
    {
        let p = link[1]
            .split('#')
            .next()
            .unwrap_or("")
            .trim_start_matches("./");
        if !p.is_empty() && !p.starts_with("https://") && !p.starts_with("http://") {
            if !exists(p) {
                r.errors.push(format!("Missing or unsafe reference: {p}"));
            } else if !r.references.iter().any(|x| x == p) {
                r.references.push(p.to_string());
            }
        }
    }
    for (p, b) in files {
        if !safe(p) {
            r.errors.push(format!("Unsafe resource path: {p}"));
        }
        if p.starts_with("scripts/")
            || [
                "sh", "bash", "ps1", "bat", "cmd", "py", "js", "mjs", "cjs", "exe", "rb", "pl",
                "psm1", "vbs", "dll", "so", "dylib",
            ]
            .contains(
                &p.rsplit('.')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str(),
            )
            || b.starts_with(b"#!")
        {
            r.scripts.push(p.clone());
            r.risks.extend(scan_script(p, b));
        }
        let s = String::from_utf8_lossy(b);
        if [
            "https://",
            "http://",
            "curl ",
            "wget ",
            "fetch(",
            "requests.",
            "socket",
            "urllib",
        ]
        .iter()
        .any(|h| s.contains(h))
        {
            r.network.push(p.clone());
        }
    }
    for script in &r.scripts {
        let mentioned = text.contains(script.as_str())
            || r.references
                .iter()
                .any(|d| d.ends_with('/') && script.starts_with(d.as_str()));
        if !mentioned && script != "SKILL.md" {
            r.warnings
                .push(format!("Script is not referenced by SKILL.md: {script}"));
        }
    }
    r.security = summarize(&r.scripts, &r.risks);
    r
}

async fn skill_files(
    gh: &Github,
    repo: &str,
    commit: &str,
    tree: &[Value],
    path: &str,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    if path != "SKILL.md" && (!safe(path) || !path.ends_with("/SKILL.md")) {
        return Err("Invalid skill path".into());
    }
    let prefix = path.strip_suffix("SKILL.md").ok_or("Invalid skill path")?;
    let entries: Vec<_> = tree
        .iter()
        .filter(|e| {
            e["path"].as_str().is_some_and(|p| p.starts_with(prefix)) && e["type"] != "tree"
        })
        .collect();
    if entries.len() > 128 {
        return Err("Skill exceeds 128 files".into());
    }
    let mut files = BTreeMap::new();
    let mut normalized = std::collections::BTreeSet::new();
    let mut total = 0;
    for e in entries {
        let p = e["path"]
            .as_str()
            .ok_or("Missing path")?
            .strip_prefix(prefix)
            .ok_or("Invalid path")?;
        if !normalized.insert(p.to_lowercase())
            || !safe(p)
            || e["type"] != "blob"
            || !matches!(e["mode"].as_str(), Some("100644" | "100755"))
            || e["size"].as_u64().unwrap_or(u64::MAX) > LIMIT as u64
        {
            return Err("Unsafe, linked or oversized skill resource".into());
        }
        total += e["size"].as_u64().unwrap_or(0);
        if total > (4 * LIMIT) as u64 {
            return Err("Skill exceeds 4 MiB".into());
        }
        files.insert(p.into(), gh.file(repo, commit, e).await?);
    }
    Ok(files)
}

/// Registry server.json metadata; transport is never guessed from a package name.
pub fn parse_mcp(v: &Value) -> Value {
    let server = v.get("server").unwrap_or(v);
    let mut configs = vec![];
    for p in server["packages"].as_array().into_iter().flatten() {
        let env: BTreeMap<String, Value> = p["environmentVariables"].as_array().into_iter().flatten().filter_map(|e| e["name"].as_str().map(|n| (n.into(), json!({"required":e["isRequired"], "secret":e["isSecret"], "description": e["description"]})))).collect();
        let placeholders: BTreeMap<_, _> = env
            .keys()
            .map(|name| (name.clone(), format!("<set {name}>")))
            .collect();
        let identifier = p["identifier"].as_str().unwrap_or("");
        let version = p["version"].as_str().unwrap_or("");
        let template = if p["transport"]["type"] == "stdio"
            && !identifier.is_empty()
            && !version.is_empty()
        {
            match p["registryType"].as_str() {
                Some("npm") => {
                    json!({"command":"npx","args":["-y", format!("{identifier}@{version}")],"env":placeholders})
                }
                Some("pypi") => {
                    json!({"command":"uvx","args":[format!("{identifier}=={version}")],"env":placeholders})
                }
                _ => Value::Null,
            }
        } else {
            Value::Null
        };
        configs.push(json!({"transport":p["transport"]["type"],"package":p["identifier"],"version":p["version"],"env":env,"arguments":p["packageArguments"],"runtimeArguments":p["runtimeArguments"],"configTemplate":template}));
    }
    for r in server["remotes"].as_array().into_iter().flatten() {
        let headers: BTreeMap<_, _> = r["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|h| {
                h["name"]
                    .as_str()
                    .map(|name| (name.to_string(), format!("<set {name}>")))
            })
            .collect();
        let template = match r["type"].as_str() {
            Some("streamable-http") => json!({"url":r["url"],"transport":"http","headers":headers}),
            Some("sse") => json!({"url":r["url"],"transport":"sse","headers":headers}),
            _ => Value::Null,
        };
        configs.push(json!({"transport":r["type"],"url":r["url"],"headers":r["headers"],"variables":r["variables"],"configTemplate":template}));
    }
    json!({"name":server["name"],"config":configs})
}

fn check(id: &str, status: &str, detail: impl Into<String>) -> Value {
    json!({"id": id, "status": status, "detail": detail.into()})
}

/// Protocol and metadata checks of one registry `server.json`, against the MCP Registry schema
/// and the transports of the MCP specification. `repo` is the GitHub repository the entry was
/// found for. Statuses are `pass`, `warn` and `fail`; nothing here contacts the server.
pub fn evaluate_mcp(v: &Value, repo: Option<&str>) -> Value {
    let server = v.get("server").unwrap_or(v);
    let mut top = vec![];
    let name = server["name"].as_str().unwrap_or("");
    let name_ok = name.split_once('/').is_some_and(|(ns, n)| {
        ns.contains('.')
            && !n.is_empty()
            && ns
                .bytes()
                .chain(n.bytes())
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    });
    top.push(if name_ok {
        check("name_format", "pass", name)
    } else {
        check("name_format", "fail", name)
    });
    if let (Some(repo), Some(owner)) = (repo, name.strip_prefix("io.github.")) {
        let owner = owner.split('/').next().unwrap_or("");
        let matches = repo
            .split('/')
            .next()
            .is_some_and(|o| o.eq_ignore_ascii_case(owner));
        top.push(check(
            "namespace_matches_repo",
            if matches { "pass" } else { "fail" },
            owner,
        ));
    }
    top.push(
        if server["description"]
            .as_str()
            .is_some_and(|d| !d.trim().is_empty())
        {
            check("description", "pass", "")
        } else {
            check("description", "warn", "")
        },
    );
    top.push(
        if server["version"]
            .as_str()
            .is_some_and(|d| !d.trim().is_empty())
        {
            check(
                "server_version",
                "pass",
                server["version"].as_str().unwrap_or(""),
            )
        } else {
            check("server_version", "warn", "")
        },
    );
    top.push(
        if server["$schema"]
            .as_str()
            .is_some_and(|u| u.contains("modelcontextprotocol.io"))
        {
            check("schema", "pass", server["$schema"].as_str().unwrap_or(""))
        } else {
            check("schema", "warn", "")
        },
    );
    let mut entries = vec![];
    for p in server["packages"].as_array().into_iter().flatten() {
        let transport = p["transport"]["type"].as_str().unwrap_or("");
        let version = p["version"].as_str().unwrap_or("");
        let mut checks = vec![];
        checks.push(if transport == "stdio" {
            check("transport", "pass", transport)
        } else if matches!(transport, "streamable-http" | "sse") {
            check("transport", "warn", transport)
        } else {
            check("transport", "fail", transport)
        });
        checks.push(
            if version.is_empty() || version.eq_ignore_ascii_case("latest") {
                check("version_pinned", "fail", version)
            } else {
                check("version_pinned", "pass", version)
            },
        );
        checks.push(
            if matches!(p["registryType"].as_str(), Some("npm" | "pypi")) {
                check(
                    "package_manager",
                    "pass",
                    p["registryType"].as_str().unwrap_or(""),
                )
            } else {
                check(
                    "package_manager",
                    "warn",
                    p["registryType"].as_str().unwrap_or(""),
                )
            },
        );
        let secrets = p["environmentVariables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["isSecret"] == true)
            .count();
        checks.push(check(
            "secrets",
            if secrets > 0 { "warn" } else { "pass" },
            secrets.to_string(),
        ));
        entries.push(json!({"kind": if transport == "stdio" {"local_stdio"} else {"unknown"}, "label": p["identifier"], "checks": checks}));
    }
    for r in server["remotes"].as_array().into_iter().flatten() {
        let transport = r["type"].as_str().unwrap_or("");
        let url = reqwest::Url::parse(r["url"].as_str().unwrap_or("")).ok();
        let mut checks = vec![];
        checks.push(match transport {
            "streamable-http" => check("transport", "pass", transport),
            "sse" => check("transport", "warn", transport),
            _ => check("transport", "fail", transport),
        });
        checks.push(match &url {
            Some(u)
                if u.scheme() == "https" && u.username().is_empty() && u.password().is_none() =>
            {
                check("remote_https", "pass", u.origin().ascii_serialization())
            }
            Some(u)
                if u.scheme() == "http"
                    && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) =>
            {
                check("remote_https", "warn", u.origin().ascii_serialization())
            }
            _ => check("remote_https", "fail", r["url"].as_str().unwrap_or("")),
        });
        let secret_headers = r["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|h| h["isSecret"] == true)
            .count();
        checks.push(check(
            "secrets",
            if secret_headers > 0 { "warn" } else { "pass" },
            secret_headers.to_string(),
        ));
        entries.push(json!({"kind": if matches!(transport, "streamable-http" | "sse") {"remote_http"} else {"unknown"}, "label": r["url"], "checks": checks}));
    }
    if entries.is_empty() {
        top.push(check("deployment", "fail", ""));
    }
    json!({"checks": top, "entries": entries})
}

pub fn parse_manifests(manifests: &BTreeMap<String, String>, readme: &str) -> Value {
    let package = manifests
        .get("package.json")
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    let python = manifests
        .get("pyproject.toml")
        .and_then(|s| toml::from_str::<toml::Value>(s).ok())
        .and_then(|v| serde_json::to_value(v).ok())
        .unwrap_or(Value::Null);
    let env_pattern = regex::Regex::new(
        r#"(?:process\.env\.|\$\{|os\.environ\[?["']?|getenv\(["'])([A-Z][A-Z0-9_]{2,})"#,
    )
    .expect("constant regex");
    let text = format!(
        "{readme}\n{}",
        manifests.values().cloned().collect::<Vec<_>>().join("\n")
    );
    let env: std::collections::BTreeSet<_> = env_pattern
        .captures_iter(&text)
        .map(|c| c[1].to_string())
        .collect();
    // Manifests identify entry points, not transport or a guarantee of required secrets.
    json!({"transport":"unknown", "npm": {"name":package["name"], "bin":package["bin"], "scripts":package["scripts"], "os":package["os"], "cpu":package["cpu"]}, "python": {"name":python["project"]["name"], "scripts":python["project"]["scripts"], "requires-python":python["project"]["requires-python"]}, "environmentHints":env})
}

#[derive(Serialize)]
pub struct Review {
    pub repo: String,
    pub sha: String,
    pub readme: String,
    pub release: Value,
    pub platforms: Vec<String>,
    pub skills: Vec<SkillReview>,
    pub mcp: Value,
    pub manifests: BTreeMap<String, String>,
}
pub async fn inspect(repo: &str) -> Result<Review, String> {
    let repo = repository(repo)?;
    let gh = Github::new()?;
    let info = gh.get(&format!("/repos/{repo}")).await?;
    if !public(&info) {
        return Err("Only public repositories are supported".into());
    }
    let branch = info["default_branch"]
        .as_str()
        .ok_or("Missing default branch")?;
    let encoded: String = reqwest::Url::parse_with_params(
        "https://api.github.com/commits",
        &[("sha", branch), ("per_page", "1")],
    )
    .map_err(|e| e.to_string())?
    .query()
    .unwrap_or_default()
    .into();
    let commit = gh.get(&format!("/repos/{repo}/commits?{encoded}")).await?;
    let sha = commit[0]["sha"]
        .as_str()
        .ok_or("Missing commit SHA")?
        .to_string();
    let tree = gh.tree(&repo, &sha).await?;
    let mut readme = String::new();
    let mut manifests = BTreeMap::new();
    let mut skills = vec![];
    for e in &tree {
        let p = e["path"].as_str().unwrap_or("");
        if p.eq_ignore_ascii_case("README.md")
            || ["package.json", "pyproject.toml", "server.json"].contains(&p)
        {
            let bytes = gh.file(&repo, &sha, e).await?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            if p.eq_ignore_ascii_case("README.md") {
                readme = text.chars().take(6000).collect();
            } else {
                manifests.insert(p.into(), text);
            }
        }
        if p == "SKILL.md" || p.ends_with("/SKILL.md") {
            if skills.len() >= 20 {
                return Err("Repository exceeds 20 skills".into());
            }
            match skill_files(&gh, &repo, &sha, &tree, p).await {
                Ok(files) => {
                    let mut review = validate_skill(p, &files);
                    review.commit = sha.clone();
                    review.install_dir = crate::skills::root()
                        .join(&review.name)
                        .to_string_lossy()
                        .into_owned();
                    skills.push(review);
                }
                Err(e) => skills.push(SkillReview {
                    path: p.into(),
                    name: String::new(),
                    errors: vec![e],
                    scripts: vec![],
                    network: vec![],
                    bytes: 0,
                    references: vec![],
                    warnings: vec![],
                    risks: vec![],
                    security: SecuritySummary::default(),
                    commit: sha.clone(),
                    scope: "user".into(),
                    install_dir: String::new(),
                }),
            }
        }
    }
    let server = manifests
        .get("server.json")
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or(Value::Null);
    let mut mcp = parse_mcp(&server);
    if !server.is_null() {
        mcp["evaluation"] = evaluate_mcp(&server, Some(&repo));
    }
    mcp["manifestHints"] = parse_manifests(&manifests, &readme);
    // Registry metadata is separate from Harness's official catalog. Never send the GitHub token.
    let registry = Github {
        client: gh.client.clone(),
        base: "https://registry.modelcontextprotocol.io".into(),
        raw: String::new(),
        token: None,
        cache: gh.cache.join("mcp-registry"),
    };
    let name = repo.split('/').next_back().unwrap_or("");
    let query: String = reqwest::Url::parse_with_params(
        "https://registry.modelcontextprotocol.io/v0.1/servers",
        &[("search", name), ("limit", "100")],
    )
    .map_err(|e| e.to_string())?
    .query()
    .unwrap_or_default()
    .into();
    match registry.get(&format!("/v0.1/servers?{query}")).await {
        Ok(data) => {
            mcp["registry"] = json!(data["servers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|s| s
                    .pointer("/server/repository/url")
                    .and_then(Value::as_str)
                    .and_then(|u| repository(u).ok())
                    .is_some_and(|r| r.eq_ignore_ascii_case(&repo)))
                .map(|s| {
                    let mut parsed = parse_mcp(s);
                    parsed["evaluation"] = evaluate_mcp(s, Some(&repo));
                    parsed
                })
                .collect::<Vec<_>>());
        }
        Err(e) => {
            mcp["registryError"] = json!(e);
        }
    }
    let hint = format!(
        "{} {}",
        readme,
        manifests.values().cloned().collect::<Vec<_>>().join(" ")
    )
    .to_lowercase();
    let platforms = ["windows", "linux", "macos", "darwin"]
        .into_iter()
        .filter(|p| hint.contains(p))
        .map(str::to_string)
        .collect();
    let release = match gh.get(&format!("/repos/{repo}/releases/latest")).await {
        Ok(v) => v,
        Err(e) if e == "GitHub returned HTTP 404" => Value::Null,
        Err(e) => return Err(e),
    };
    Ok(Review {
        repo,
        sha,
        readme,
        release,
        platforms,
        skills,
        mcp,
        manifests,
    })
}

pub async fn install(
    repo: &str,
    sha: &str,
    path: &str,
    scripts: &[String],
    security_level: &str,
    confirmed: bool,
    root: &Path,
) -> Result<(), String> {
    if !confirmed || sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Confirm the script list and a pinned commit SHA".into());
    }
    let repo = repository(repo)?;
    let gh = Github::new()?;
    let info = gh.get(&format!("/repos/{repo}")).await?;
    if !public(&info) {
        return Err("Only public repositories are supported".into());
    }
    let tree = gh.tree(&repo, sha).await?;
    let files = skill_files(&gh, &repo, sha, &tree, path).await?;
    let review = validate_skill(path, &files);
    if !review.errors.is_empty()
        || review.scripts != scripts
        || review.security.level != security_level
    {
        return Err("Skill validation or script confirmation failed; review again".into());
    }
    install_files(root, &repo, sha, &review, &files)
}
fn install_files(
    root: &Path,
    repo: &str,
    sha: &str,
    review: &SkillReview,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let target = root.join(&review.name);
    if target.exists() {
        return Err("Skill already exists; remove it before installing".into());
    }
    let staging = tempfile::Builder::new()
        .prefix(".github-skill-")
        .tempdir_in(root)
        .map_err(|e| e.to_string())?;
    for (p, b) in files {
        let dest = staging.path().join(p);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, b).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        staging.path().join(".github-source.json"),
        serde_json::to_vec(&json!({"repository":repo,"commit":sha})).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    crate::skills::disable_for_install(root, &review.name)?;
    std::fs::rename(staging.path(), &target).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const SKILL: &str = "---\nname: demo\ndescription: Demonstrate a skill.\n---\nRun scripts/demo.py and read [guide](references/guide.md).";
    fn files() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            ("SKILL.md".into(), SKILL.as_bytes().to_vec()),
            (
                "scripts/demo.py".into(),
                b"requests.get('https://example.com')".to_vec(),
            ),
            ("references/guide.md".into(), b"Guide".to_vec()),
        ])
    }
    async fn mock(status: &str, headers: &str, body: &str, cache: &Path) -> Github {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 8192];
            let len = socket.read(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..len]).to_lowercase();
            assert!(!request.contains("authorization:"));
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        Github {
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap(),
            base: format!("http://{addr}"),
            raw: format!("http://{addr}"),
            token: None,
            cache: cache.into(),
        }
    }
    #[tokio::test]
    async fn mocked_search_parsing_and_disk_ttl() {
        let temp = tempfile::tempdir().unwrap();
        let gh = mock("200 OK", "", r#"{"items":[{"full_name":"owner/demo","private":false},{"full_name":"owner/private","private":true},{"private":false}]}"#, temp.path()).await;
        let data = gh.get("/search/repositories?q=demo").await.unwrap();
        assert_eq!(parse_search(&data).unwrap().len(), 1);
        assert!(parse_search(&json!({"message":"bad"})).is_err());
        // The one-shot mock is gone: the next read must be the disk cache.
        assert_eq!(gh.get("/search/repositories?q=demo").await.unwrap(), data);
        for entry in std::fs::read_dir(temp.path()).unwrap().flatten() {
            let store = crate::store::JsonStore::new(entry.path());
            if let Ok(mut c) = store.load::<Cache>() {
                c.expires = 0;
                store.save(&c).unwrap();
            }
        }
        assert!(gh.get("/search/repositories?q=demo").await.is_err());
    }
    #[tokio::test]
    async fn mocked_rate_limit_persists_across_queries() {
        let temp = tempfile::tempdir().unwrap();
        let gh = mock(
            "429 Too Many Requests",
            "Retry-After: 120\r\n",
            "{}",
            temp.path(),
        )
        .await;
        assert!(gh
            .get("/search/repositories?q=one")
            .await
            .unwrap_err()
            .contains("rate limit"));
        assert!(gh
            .get("/search/repositories?q=two")
            .await
            .unwrap_err()
            .contains("cooldown"));
    }
    #[tokio::test]
    async fn mocked_skill_blob_validation_and_resources() {
        let temp = tempfile::tempdir().unwrap();
        let body =
            json!({"content":base64::engine::general_purpose::STANDARD.encode(SKILL)}).to_string();
        let gh = mock("200 OK", "", &body, temp.path()).await;
        let mut content = files();
        content.insert(
            "SKILL.md".into(),
            gh.blob("owner/demo", "abc").await.unwrap(),
        );
        let r = validate_skill("skills/demo/SKILL.md", &content);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(r.scripts, vec!["scripts/demo.py"]);
        assert_eq!(r.network, vec!["scripts/demo.py"]);
        content.remove("references/guide.md");
        assert!(!validate_skill("SKILL.md", &content).errors.is_empty());
        content.insert("SKILL.md".into(), b"No frontmatter".to_vec());
        assert!(!validate_skill("SKILL.md", &content).errors.is_empty());
        content.insert("huge".into(), vec![0; LIMIT + 1]);
        assert!(validate_skill("SKILL.md", &content)
            .errors
            .iter()
            .any(|e| e.contains("1 MiB")));
    }
    #[tokio::test]
    async fn skill_files_come_from_the_pinned_raw_address_checked_by_size() {
        let temp = tempfile::tempdir().unwrap();
        let entry = json!({"path":"demo/SKILL.md","type":"blob","sha":"abc","size":5});
        let gh = mock("200 OK", "", "hello", temp.path()).await;
        assert_eq!(
            gh.file("owner/demo", "c0ffee", &entry).await.unwrap(),
            b"hello"
        );
        // A body of another size is not this commit's file; the API fallback (gone here) fails.
        let gh = mock("200 OK", "", "hello!", temp.path()).await;
        assert!(gh.file("owner/demo", "c0ffee", &entry).await.is_err());
    }
    #[tokio::test]
    async fn mocked_registry_mcp_metadata_preserves_transports_and_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let metadata = json!({"server":{"name":"io.example/demo","packages":[{"registryType":"npm","identifier":"@demo/server","version":"1.2.3","transport":{"type":"stdio"},"environmentVariables":[{"name":"API_KEY","isRequired":true,"isSecret":true}]}],"remotes":[{"type":"streamable-http","url":"https://example.com/mcp","headers":[{"name":"Authorization","isRequired":true,"isSecret":true}]}]}});
        let gh = mock("200 OK", "", &metadata.to_string(), temp.path()).await;
        let parsed = parse_mcp(&gh.get("/v0.1/servers/demo").await.unwrap());
        assert_eq!(parsed["config"][0]["transport"], "stdio");
        assert_eq!(parsed["config"][0]["env"]["API_KEY"]["secret"], true);
        assert_eq!(
            parsed["config"][0]["configTemplate"]["args"][1],
            "@demo/server@1.2.3"
        );
        assert_eq!(parsed["config"][1]["transport"], "streamable-http");
        assert_eq!(parsed["config"][1]["headers"][0]["isRequired"], true);
        assert_eq!(parse_mcp(&json!({}))["config"], json!([]));
    }
    #[test]
    fn package_and_pyproject_entrypoints_do_not_invent_transport() {
        let manifests = BTreeMap::from([
            ("package.json".into(),r#"{"name":"demo","bin":{"demo":"cli.js"},"scripts":{"postinstall":"node install.js"},"os":["linux"]}"#.into()),
            ("pyproject.toml".into(),"[project]\nname = 'demo-python'\n[project.scripts]\ndemo = 'demo:main'".into()),
        ]);
        let parsed = parse_manifests(&manifests, "Set ${API_KEY} and process.env.SERVICE_URL");
        assert_eq!(parsed["transport"], "unknown");
        assert_eq!(parsed["python"]["scripts"]["demo"], "demo:main");
        assert_eq!(parsed["npm"]["scripts"]["postinstall"], "node install.js");
        assert_eq!(
            parsed["environmentHints"],
            json!(["API_KEY", "SERVICE_URL"])
        );
    }
    #[test]
    fn directory_and_external_references_are_valid_but_local_missing_links_are_not() {
        let mut content = files();
        let text = "---\nname: demo\ndescription: Demonstrate a skill.\n---\nSee references/ and [external](https://example.com/references/remote.md).";
        content.insert("SKILL.md".into(), text.as_bytes().to_vec());
        assert!(validate_skill("SKILL.md", &content).errors.is_empty());
        content.insert(
            "SKILL.md".into(),
            format!("{text}\n[missing](missing.md \"Title\")\n[ref]: missing-too.md").into_bytes(),
        );
        assert_eq!(validate_skill("SKILL.md", &content).errors.len(), 2);
    }
    #[tokio::test]
    async fn incomplete_trees_and_symlinks_are_refused_before_reading_resources() {
        let temp = tempfile::tempdir().unwrap();
        let gh = mock("200 OK", "", r#"{"truncated":true,"tree":[]}"#, temp.path()).await;
        assert!(gh
            .tree("owner/demo", "abc")
            .await
            .unwrap_err()
            .contains("incomplete"));
        let tree = vec![
            json!({"path":"demo/SKILL.md","type":"blob","mode":"120000","sha":"abc","size":10}),
        ];
        assert!(
            skill_files(&gh, "owner/demo", "abc", &tree, "demo/SKILL.md")
                .await
                .unwrap_err()
                .contains("linked")
        );
    }
    #[test]
    fn url_and_resource_confinement_and_disabled_install() {
        assert_eq!(
            repository("https://github.com/owner/repo.git").unwrap(),
            "owner/repo"
        );
        for u in [
            "https://github.com.evil/owner/repo",
            "https://user@github.com/owner/repo",
            "https://github.com/owner/repo/tree/main",
            "owner/..",
        ] {
            assert!(repository(u).is_err(), "{u}");
        }
        for p in [
            "../secret",
            "x/../../secret",
            "C:/secret",
            "x\\secret",
            "CON.txt",
            "x/.",
        ] {
            assert!(!safe(p), "{p}");
        }
        let temp = tempfile::tempdir().unwrap();
        let content = files();
        let review = validate_skill("SKILL.md", &content);
        install_files(
            temp.path(),
            "owner/demo",
            &"a".repeat(40),
            &review,
            &content,
        )
        .unwrap();
        assert!(!crate::skills::list(temp.path())[0].enabled);
        assert_eq!(
            std::fs::read(temp.path().join("demo/scripts/demo.py")).unwrap(),
            content["scripts/demo.py"]
        );
        assert!(install_files(
            temp.path(),
            "owner/demo",
            &"a".repeat(40),
            &review,
            &content
        )
        .is_err());
    }
    #[tokio::test]
    async fn install_requires_confirmation_before_any_network_or_keychain_access() {
        let temp = tempfile::tempdir().unwrap();
        assert!(install(
            "owner/demo",
            &"a".repeat(40),
            "SKILL.md",
            &[],
            "none",
            false,
            temp.path()
        )
        .await
        .is_err());
        assert!(install(
            "owner/demo",
            "main",
            "SKILL.md",
            &[],
            "none",
            true,
            temp.path()
        )
        .await
        .is_err());
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    #[test]
    fn script_scan_flags_network_shell_eval_and_credential_patterns() {
        let script = b"import subprocess, requests\nrequests.get('https://x.test')\nsubprocess.run(['ls'])\neval(data)\nopen('~/.ssh/id_rsa')\n";
        let risks = scan_script("scripts/a.py", script);
        let cats: std::collections::BTreeSet<_> =
            risks.iter().map(|r| r.category.as_str()).collect();
        assert_eq!(
            cats,
            ["credentials", "eval", "network", "shell"]
                .into_iter()
                .collect()
        );
        assert!(risks.iter().any(|r| r.category == "eval" && r.line == 4));
        assert!(scan_script("scripts/ok.py", b"print('hi')\n").is_empty());
        let binary = scan_script("scripts/tool.exe", b"MZ\0\0");
        assert_eq!(binary[0].category, "binary");
        let summary = summarize(&["scripts/a.py".into()], &risks);
        assert_eq!(summary.level, "high");
        assert_eq!(summarize(&[], &[]).level, "none");
        assert_eq!(summarize(&["a.sh".into()], &[]).level, "low");
        let net = scan_script("a.sh", b"curl https://x.test");
        assert_eq!(summarize(&["a.sh".into()], &net).level, "medium");
    }
    #[test]
    fn skill_review_reports_references_warnings_security_and_pins_the_security_level() {
        let mut content = files();
        content.insert("scripts/unused.sh".into(), b"echo hi".to_vec());
        let review = validate_skill("SKILL.md", &content);
        assert!(review.errors.is_empty(), "{:?}", review.errors);
        assert!(review
            .references
            .contains(&"references/guide.md".to_string()));
        assert!(review.references.contains(&"scripts/demo.py".to_string()));
        assert_eq!(
            review.warnings,
            vec!["Script is not referenced by SKILL.md: scripts/unused.sh"]
        );
        assert_eq!(review.security.level, "medium");
        assert_eq!(review.security.network, 2);
        assert_eq!(review.scope, "user");
    }
    #[test]
    fn mcp_evaluation_checks_protocol_metadata_and_transport_kind() {
        let good = json!({"server":{"$schema":"https://static.modelcontextprotocol.io/schemas/2025-07-09/server.schema.json","name":"io.github.owner/demo","description":"Demo","version":"1.0.0","packages":[{"registryType":"npm","identifier":"@demo/server","version":"1.2.3","transport":{"type":"stdio"},"environmentVariables":[{"name":"K","isSecret":true}]}],"remotes":[{"type":"streamable-http","url":"https://example.com/mcp"}]}});
        let e = evaluate_mcp(&good, Some("Owner/demo"));
        let status = |checks: &Value, id: &str| {
            checks
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .map(|c| c["status"].as_str().unwrap().to_string())
        };
        assert_eq!(status(&e["checks"], "name_format").as_deref(), Some("pass"));
        assert_eq!(
            status(&e["checks"], "namespace_matches_repo").as_deref(),
            Some("pass")
        );
        assert_eq!(e["entries"][0]["kind"], "local_stdio");
        assert_eq!(
            status(&e["entries"][0]["checks"], "version_pinned").as_deref(),
            Some("pass")
        );
        assert_eq!(
            status(&e["entries"][0]["checks"], "secrets").as_deref(),
            Some("warn")
        );
        assert_eq!(e["entries"][1]["kind"], "remote_http");
        assert_eq!(
            status(&e["entries"][1]["checks"], "remote_https").as_deref(),
            Some("pass")
        );
        let bad = json!({"server":{"name":"demo","packages":[{"registryType":"npm","identifier":"x","version":"latest","transport":{"type":"stdio"}}],"remotes":[{"type":"sse","url":"http://example.com/mcp"}]}});
        let e = evaluate_mcp(&bad, Some("other/demo"));
        assert_eq!(status(&e["checks"], "name_format").as_deref(), Some("fail"));
        assert_eq!(
            status(&e["entries"][0]["checks"], "version_pinned").as_deref(),
            Some("fail")
        );
        assert_eq!(
            status(&e["entries"][1]["checks"], "remote_https").as_deref(),
            Some("fail")
        );
        assert_eq!(
            status(&e["entries"][1]["checks"], "transport").as_deref(),
            Some("warn")
        );
        let mismatch = evaluate_mcp(
            &json!({"server":{"name":"io.github.evil/demo"}}),
            Some("owner/demo"),
        );
        assert_eq!(
            status(&mismatch["checks"], "namespace_matches_repo").as_deref(),
            Some("fail")
        );
        assert_eq!(
            status(&mismatch["checks"], "deployment").as_deref(),
            Some("fail")
        );
    }
}
