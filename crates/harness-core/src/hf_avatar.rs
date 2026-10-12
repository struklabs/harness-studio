//! Hugging Face publisher avatars.
//!
//! HF exposes `GET /api/organizations/{name}/avatar` and `GET /api/users/{name}/avatar`, both
//! returning `{"avatarUrl": "https://cdn-avatars.huggingface.co/..."}` (404 when the name is not
//! of that kind). Model search results carry no avatar, so we resolve them per author and keep
//! the answers in memory plus a small JSON file so the UI never spams huggingface.co.
//!
//! Privacy: only `huggingface.co` is contacted and no token is ever sent.

use crate::error_key::err;
use crate::paths;
use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const SCHEMA_VERSION: u32 = 1;
/// Found avatars change rarely.
const HIT_TTL_SECS: u64 = 7 * 24 * 3600;
/// "No avatar" answers are re-checked sooner (a publisher may add one).
const MISS_TTL_SECS: u64 = 24 * 3600;
const MAX_AUTHORS_PER_CALL: usize = 60;
const CONCURRENCY: usize = 6;
const USER_AGENT: &str = "HarnessStudio/0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvatarEntry {
    pub url: Option<String>,
    pub fetched_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AvatarCache {
    pub schema_version: u32,
    pub entries: HashMap<String, AvatarEntry>,
}

impl Default for AvatarCache {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            entries: HashMap::new(),
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl AvatarCache {
    /// Missing, unreadable or wrong-version files start an empty cache.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<AvatarCache>(&s).ok())
            .filter(|c| c.schema_version == SCHEMA_VERSION)
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                String::from(
                    err("fs.create_dir_failed")
                        .with("path", dir.display())
                        .with("error", e),
                )
            })?;
        }
        let json = serde_json::to_string(self)
            .map_err(|e| String::from(err("json.serialize_failed").with("error", e)))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| {
            String::from(
                err("fs.write_failed")
                    .with("path", tmp.display())
                    .with("error", e),
            )
        })?;
        std::fs::rename(&tmp, path).map_err(|e| {
            err("fs.rename_failed")
                .with("from", tmp.display())
                .with("to", path.display())
                .with("error", e)
                .into()
        })
    }

    /// `Some(answer)` while the entry is fresh (the answer itself may be "no avatar").
    pub fn fresh(&self, author: &str, now: u64) -> Option<Option<String>> {
        let e = self.entries.get(&author.to_lowercase())?;
        let ttl = if e.url.is_some() {
            HIT_TTL_SECS
        } else {
            MISS_TTL_SECS
        };
        (now.saturating_sub(e.fetched_at) < ttl).then(|| e.url.clone())
    }

    pub fn put(&mut self, author: &str, url: Option<String>, now: u64) {
        self.entries.insert(
            author.to_lowercase(),
            AvatarEntry {
                url,
                fetched_at: now,
            },
        );
    }
}

/// Extracts a trusted avatar URL from an HF avatar API response body. Only https URLs on
/// huggingface.co / hf.co (and subdomains such as `cdn-avatars.huggingface.co`) are accepted.
pub fn parse_avatar_response(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let url = v.get("avatarUrl")?.as_str()?;
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let trusted = host == "huggingface.co"
        || host.ends_with(".huggingface.co")
        || host == "hf.co"
        || host.ends_with(".hf.co");
    (parsed.scheme() == "https" && trusted).then(|| url.to_string())
}

/// HF user / organization names: letters, digits, `-`, `_`, `.`.
fn valid_author(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 96
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn cache_file() -> PathBuf {
    paths::data_root().join("cache").join("hf_avatars.json")
}

enum Lookup {
    Found(String),
    NotFound,
    Failed,
}

async fn fetch_kind(client: &reqwest::Client, kind: &str, author: &str) -> Lookup {
    let url = format!("https://huggingface.co/api/{kind}/{author}/avatar");
    let Ok(res) = client.get(&url).send().await else {
        return Lookup::Failed;
    };
    let status = res.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Lookup::NotFound;
    }
    if !status.is_success() {
        return Lookup::Failed;
    }
    match res.text().await {
        Ok(body) => parse_avatar_response(&body)
            .map(Lookup::Found)
            .unwrap_or(Lookup::NotFound),
        Err(_) => Lookup::Failed,
    }
}

/// `Ok(None)`: the author has no avatar (cacheable). `Err`: transient failure (not cached).
async fn fetch_author(client: &reqwest::Client, author: &str) -> Result<Option<String>, ()> {
    let mut failed = false;
    for kind in ["organizations", "users"] {
        match fetch_kind(client, kind, author).await {
            Lookup::Found(u) => return Ok(Some(u)),
            Lookup::NotFound => {}
            Lookup::Failed => failed = true,
        }
    }
    if failed {
        Err(())
    } else {
        Ok(None)
    }
}

struct Service {
    cache: Mutex<Option<AvatarCache>>,
}

fn service() -> &'static Service {
    static S: OnceLock<Service> = OnceLock::new();
    S.get_or_init(|| Service {
        cache: Mutex::new(None),
    })
}

/// Resolves avatar URLs for the given HF authors (keys use the requested spelling).
/// Unknown / failed lookups map to `None`.
pub async fn get_avatars(authors: Vec<String>) -> HashMap<String, Option<String>> {
    let mut wanted: Vec<String> = Vec::new();
    for a in authors {
        if valid_author(&a) && !wanted.iter().any(|w| w.eq_ignore_ascii_case(&a)) {
            wanted.push(a);
        }
        if wanted.len() >= MAX_AUTHORS_PER_CALL {
            break;
        }
    }
    let path = cache_file();
    let now = now_secs();
    let mut result: HashMap<String, Option<String>> = HashMap::new();
    let mut missing: Vec<String> = Vec::new();
    {
        let mut guard = service().cache.lock().unwrap_or_else(|e| e.into_inner());
        let cache = guard.get_or_insert_with(|| AvatarCache::load(&path));
        for a in &wanted {
            match cache.fresh(a, now) {
                Some(url) => {
                    result.insert(a.clone(), url);
                }
                None => missing.push(a.clone()),
            }
        }
    }
    if missing.is_empty() {
        return result;
    }
    let Ok(client) = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(8))
        .build()
    else {
        return result;
    };
    let fetched: Vec<(String, Result<Option<String>, ()>)> = stream::iter(missing)
        .map(|a| {
            let client = client.clone();
            async move {
                let r = fetch_author(&client, &a).await;
                (a, r)
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    let mut guard = service().cache.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(|| AvatarCache::load(&path));
    for (a, r) in fetched {
        match r {
            Ok(url) => {
                cache.put(&a, url.clone(), now);
                result.insert(a, url);
            }
            Err(()) => {
                result.insert(a, None);
            }
        }
    }
    let _ = cache.save(&path);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real responses captured from huggingface.co (see tests/data).
    const ORG: &str = include_str!("../tests/data/hf_avatar_org_unsloth.json");
    const USER: &str = include_str!("../tests/data/hf_avatar_user_bartowski.json");
    const NOT_FOUND: &str = include_str!("../tests/data/hf_avatar_notfound.json");

    #[test]
    fn parses_real_org_and_user_responses() {
        let org = parse_avatar_response(ORG).unwrap();
        assert!(org.starts_with("https://cdn-avatars.huggingface.co/"));
        assert!(org.ends_with(".png"));
        let user = parse_avatar_response(USER).unwrap();
        assert!(user.ends_with(".jpeg"));
    }

    #[test]
    fn rejects_not_found_and_untrusted_hosts() {
        assert_eq!(parse_avatar_response(NOT_FOUND), None);
        assert_eq!(parse_avatar_response("not json"), None);
        assert_eq!(
            parse_avatar_response(r#"{"avatarUrl":"https://evil.example/a.png"}"#),
            None
        );
        assert_eq!(
            parse_avatar_response(r#"{"avatarUrl":"http://cdn-avatars.huggingface.co/a.png"}"#),
            None
        );
        assert_eq!(
            parse_avatar_response(r#"{"avatarUrl":"https://huggingface.co.evil.example/a.png"}"#),
            None
        );
    }

    #[test]
    fn validates_author_names() {
        assert!(valid_author("unsloth"));
        assert!(valid_author("Qwen-2.5_x"));
        assert!(!valid_author(""));
        assert!(!valid_author("a/b"));
        assert!(!valid_author("a b"));
        assert!(!valid_author("../x"));
    }

    #[test]
    fn cache_round_trips_through_a_real_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cache").join("hf_avatars.json");
        let mut c = AvatarCache::load(&path);
        assert!(c.entries.is_empty());
        c.put("Unsloth", parse_avatar_response(ORG), 1000);
        c.put("nobody", None, 1000);
        c.save(&path).unwrap();
        let back = AvatarCache::load(&path);
        assert_eq!(back, c);
        assert_eq!(back.schema_version, 1);
    }

    #[test]
    fn cache_ttl_hit_and_miss_differ() {
        let mut c = AvatarCache::default();
        c.put(
            "a",
            Some("https://cdn-avatars.huggingface.co/x.png".into()),
            0,
        );
        c.put("b", None, 0);
        assert!(c.fresh("A", 100).is_some_and(|u| u.is_some()));
        assert_eq!(c.fresh("b", 100), Some(None));
        // a miss expires after a day, a hit after a week
        assert_eq!(c.fresh("b", MISS_TTL_SECS + 1), None);
        assert!(c.fresh("a", MISS_TTL_SECS + 1).is_some());
        assert_eq!(c.fresh("a", HIT_TTL_SECS + 1), None);
        assert_eq!(c.fresh("unknown", 1), None);
    }

    #[test]
    fn wrong_schema_version_or_garbage_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.json");
        std::fs::write(
            &path,
            r#"{"schema_version":99,"entries":{"x":{"url":null,"fetched_at":1}}}"#,
        )
        .unwrap();
        assert!(AvatarCache::load(&path).entries.is_empty());
        std::fs::write(&path, "garbage").unwrap();
        assert!(AvatarCache::load(&path).entries.is_empty());
    }
}
