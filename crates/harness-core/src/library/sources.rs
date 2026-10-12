//! Where a downloaded model file came from (`model_sources.json`).
//!
//! The downloader records the URL of every file it finishes (Hugging Face `resolve` URLs and
//! custom URLs alike). Repairing a split model whose folder is not a Hugging Face repo folder
//! (custom-URL downloads, files moved elsewhere in the library) then works from the URL of a
//! part that is still there: the missing parts are the same URL with the part number replaced
//! ([`part_url`]). Files that were copied by hand have no record; the user can give the URL of
//! one part instead.
//!
//! Query strings, fragments and credentials are never stored (they may hold signatures or
//! tokens). Hugging Face resolve addresses are rebuilt from their repo, revision and file path,
//! so a fresh signed redirect is obtained during repair.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error_key::err;
use crate::store::{Envelope, JsonStore, Schema};

/// `model_sources.json`: `{schema_version, sources: {<normalized path>: {...}}}`.
pub const SOURCES_SCHEMA: Schema = Schema {
    file: "model_sources.json",
    current: 1,
    envelope: Envelope::Wrapped("sources"),
    migrations: &[],
};

/// Most records kept (oldest dropped first); a record is ~200 bytes.
const MAX_RECORDS: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSource {
    /// File URL without query / fragment / credentials.
    pub url: String,
    /// A custom URL download (not a Hugging Face repo file).
    #[serde(default)]
    pub custom: bool,
    /// Unix seconds.
    #[serde(default)]
    pub at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfResolveSource {
    pub repo_id: String,
    pub revision: String,
    pub filename: String,
}

/// Parses a Hugging Face `/<owner>/<repo>/resolve/<revision>/<file>` address. Signed query
/// parameters are deliberately ignored; the returned fields can create a fresh resolve URL.
pub fn hf_resolve_source(url: &str) -> Option<HfResolveSource> {
    let u = reqwest::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    if u.scheme() != "https" || !matches!(host.as_str(), "huggingface.co" | "hf.co") {
        return None;
    }
    let parts: Vec<String> = u.path_segments()?.map(percent_decode).collect();
    if parts.len() < 5 || parts[2] != "resolve" {
        return None;
    }
    let repo_id = format!("{}/{}", parts[0], parts[1]);
    crate::downloader::DownloadManager::validate_repo_id(&repo_id).ok()?;
    let revision = parts[3].clone();
    let filename = parts[4..].join("/");
    if revision.is_empty()
        || revision == ".."
        || filename.is_empty()
        || filename.split('/').any(|p| p.is_empty() || p == "..")
    {
        return None;
    }
    Some(HfResolveSource {
        repo_id,
        revision,
        filename,
    })
}

/// Canonical, unsigned resolve URL. Hugging Face will issue a fresh signed storage redirect.
pub fn hf_resolve_url(source: &HfResolveSource) -> Option<String> {
    let mut url = reqwest::Url::parse("https://huggingface.co/").ok()?;
    {
        let mut segments = url.path_segments_mut().ok()?;
        for segment in source.repo_id.split('/') {
            segments.push(segment);
        }
        segments.push("resolve").push(&source.revision);
        for segment in source.filename.split('/') {
            segments.push(segment);
        }
    }
    Some(url.to_string())
}

pub fn fresh_hf_resolve_url(url: &str) -> Option<String> {
    hf_resolve_url(&hf_resolve_source(url)?)
}

fn store() -> JsonStore {
    JsonStore::new(crate::paths::config_dir().join("model_sources.json"))
}

/// The URL as it is stored: scheme + host + path only.
pub fn sanitize_url(url: &str) -> Option<String> {
    let mut u = reqwest::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    u.set_query(None);
    u.set_fragment(None);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    Some(u.to_string())
}

/// Remembers that `path` was downloaded from `url`. Best effort for the caller.
pub fn record(path: &Path, url: &str, custom: bool) -> Result<(), String> {
    let Some(url) = sanitize_url(url) else {
        return Ok(());
    };
    let key = super::state_key(path);
    store().update_versioned::<HashMap<String, ModelSource>, _>(&SOURCES_SCHEMA, |map| {
        map.insert(
            key,
            ModelSource {
                url,
                custom,
                at: crate::capabilities::now_secs(),
            },
        );
        if map.len() > MAX_RECORDS {
            let mut by_age: Vec<(String, u64)> =
                map.iter().map(|(k, v)| (k.clone(), v.at)).collect();
            by_age.sort_by_key(|(_, at)| *at);
            let extra = map.len() - MAX_RECORDS;
            for (k, _) in by_age.into_iter().take(extra) {
                map.remove(&k);
            }
        }
        Ok(())
    })
}

/// Every record, keyed by [`super::state_key`]. Empty when the file is missing or unreadable.
pub fn all() -> HashMap<String, ModelSource> {
    store()
        .load_versioned_or_default::<HashMap<String, ModelSource>>(&SOURCES_SCHEMA)
        .map(|v| v.data)
        .unwrap_or_default()
}

/// The record of one file.
pub fn lookup(path: &Path) -> Option<ModelSource> {
    all().remove(&super::state_key(path))
}

/// The files moved (`migrate_model`): their records follow them.
pub fn renamed(pairs: &[(&Path, &Path)]) {
    let moves: Vec<(String, String)> = pairs
        .iter()
        .map(|(a, b)| (super::state_key(a), super::state_key(b)))
        .collect();
    let result =
        store().update_versioned::<HashMap<String, ModelSource>, _>(&SOURCES_SCHEMA, |map| {
            for (from, to) in &moves {
                if let Some(v) = map.remove(from) {
                    map.insert(to.clone(), v);
                }
            }
            Ok(())
        });
    if let Err(e) = result {
        eprintln!("[Harness Studio] Model kaynak kaydı taşınamadı: {}", e);
    }
}

/// `(stem, index, total)` of a `<stem>-00002-of-00005.gguf` file name (stem lower-cased).
pub fn shard_parts(file_name: &str) -> Option<(String, u32, u32)> {
    let lower = file_name.to_lowercase();
    let stem = lower.strip_suffix(".gguf")?;
    let mut it = stem.rsplitn(4, '-');
    let total = it.next()?;
    if it.next()? != "of" {
        return None;
    }
    let index = it.next()?;
    let base = it.next()?;
    if total.len() != 5 || index.len() != 5 {
        return None;
    }
    Some((base.to_string(), index.parse().ok()?, total.parse().ok()?))
}

/// URL of part `wanted` (a file name such as `m-00003-of-00005.gguf`) next to the part that
/// `base_url` points to. Only the last path segment changes, and only when both names are
/// parts of the same set (same stem and total): an address cannot be turned into something
/// unrelated.
pub fn part_url(base_url: &str, wanted: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(base_url).map_err(|e| {
        String::from(
            err("downloads.invalid_url")
                .with("url", base_url)
                .with("error", e),
        )
    })?;
    let segment = url
        .path_segments()
        .and_then(|mut s| s.next_back().map(str::to_string))
        .unwrap_or_default();
    let decoded = percent_decode(&segment);
    let (Some(a), Some(b)) = (shard_parts(&decoded), shard_parts(wanted)) else {
        return Err(err("library.repair_url_not_a_part")
            .with("url", base_url)
            .into());
    };
    if a.0 != b.0 || a.2 != b.2 {
        return Err(err("library.repair_url_other_set")
            .with("url", base_url)
            .with("file", wanted)
            .into());
    }
    let new_segment = replace_ci(
        &decoded,
        &format!("{:05}-of-", a.1),
        &format!("{:05}-of-", b.1),
    );
    {
        let mut segs = url
            .path_segments_mut()
            .map_err(|_| String::from(err("downloads.invalid_url").with("url", base_url)))?;
        segs.pop().push(&new_segment);
    }
    Ok(url.to_string())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Replaces the last case-insensitive occurrence of `from` in `text`.
fn replace_ci(text: &str, from: &str, to: &str) -> String {
    match text.to_lowercase().rfind(from) {
        Some(i) => format!("{}{}{}", &text[..i], to, &text[i + from.len()..]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_lose_query_fragment_and_credentials() {
        assert_eq!(
            sanitize_url("https://user:pw@example.com/a/m.gguf?token=abc&X-Amz-Signature=z#frag")
                .unwrap(),
            "https://example.com/a/m.gguf"
        );
        assert!(sanitize_url("ftp://example.com/m.gguf").is_none());
        assert!(sanitize_url("not a url").is_none());
    }

    #[test]
    fn signed_hf_resolve_url_is_rebuilt_with_repo_revision_and_filename() {
        let signed = "https://huggingface.co/acme/model/resolve/refs%2Fpr%2F7/sub/Model-00001-of-00003.gguf?X-Amz-Signature=secret";
        let source = hf_resolve_source(signed).unwrap();
        assert_eq!(source.repo_id, "acme/model");
        assert_eq!(source.revision, "refs/pr/7");
        assert_eq!(source.filename, "sub/Model-00001-of-00003.gguf");
        assert_eq!(
            hf_resolve_url(&source).unwrap(),
            "https://huggingface.co/acme/model/resolve/refs%2Fpr%2F7/sub/Model-00001-of-00003.gguf"
        );
    }

    #[test]
    fn shard_names_parse_strictly() {
        assert_eq!(
            shard_parts("Qwen3-235B-A22B-Q4_K_M-00002-of-00005.gguf"),
            Some(("qwen3-235b-a22b-q4_k_m".to_string(), 2, 5))
        );
        assert_eq!(shard_parts("model.gguf"), None);
        assert_eq!(shard_parts("m-2-of-5.gguf"), None);
    }

    #[test]
    fn part_urls_swap_only_the_part_number() {
        let base = "https://example.com/files/Big-Q4_K_M-00001-of-00003.gguf";
        assert_eq!(
            part_url(base, "Big-Q4_K_M-00003-of-00003.gguf").unwrap(),
            "https://example.com/files/Big-Q4_K_M-00003-of-00003.gguf"
        );
        // Hugging Face style address with an encoded name and a sub-folder.
        let hf =
            "https://huggingface.co/o/r/resolve/main/Q4_K_M/Big%2BX-Q4_K_M-00001-of-00003.gguf";
        assert_eq!(
            part_url(hf, "Big+X-Q4_K_M-00002-of-00003.gguf").unwrap(),
            "https://huggingface.co/o/r/resolve/main/Q4_K_M/Big+X-Q4_K_M-00002-of-00003.gguf"
        );
        // A different set or a single file is refused.
        for (u, wanted, key) in [
            (
                base,
                "Other-Q4_K_M-00002-of-00003.gguf",
                "library.repair_url_other_set",
            ),
            (
                base,
                "Big-Q4_K_M-00002-of-00004.gguf",
                "library.repair_url_other_set",
            ),
            (
                "https://example.com/Big.gguf",
                "Big-Q4_K_M-00002-of-00003.gguf",
                "library.repair_url_not_a_part",
            ),
        ] {
            let e = part_url(u, wanted).unwrap_err();
            assert!(e.starts_with(key), "{e}");
        }
    }

    #[test]
    fn records_round_trip_follow_a_move_and_stay_bounded() {
        let a = Path::new("C:/lib/x/m-00001-of-00002.gguf");
        let b = Path::new("D:/other/m-00001-of-00002.gguf");
        record(
            a,
            "https://example.com/m-00001-of-00002.gguf?sig=secret",
            true,
        )
        .unwrap();
        let got = lookup(a).unwrap();
        assert_eq!(got.url, "https://example.com/m-00001-of-00002.gguf");
        assert!(got.custom && got.at > 0);
        renamed(&[(a, b)]);
        assert!(lookup(a).is_none());
        assert_eq!(lookup(b).unwrap().url, got.url);
        assert!(lookup(Path::new("C:/never/seen.gguf")).is_none());
    }
}
