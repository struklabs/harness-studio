//! Installing a runtime from the GitHub releases of a llama.cpp fork (or ggml-org itself).
//!
//! Release archives follow llama.cpp's naming, `llama-<tag>-bin-<os>-<backend>[-<ver>]-<arch>`
//! (`.zip` on Windows, `.tar.gz` elsewhere); PrismML's fork uses the same scheme with tags
//! like `prism-b10743-adfffbe`. Only archives of the named repository's own releases are
//! downloaded (`https://github.com/<repo>/releases/download/...`), each one checked against
//! the SHA-256 digest GitHub publishes for it. An archive without a digest is refused
//! ([`crate::runtime::ERR_INTEGRITY_UNVERIFIED`]) unless the user confirmed it.

use super::{RuntimeEntry, RuntimeRegistry, RuntimeSource, RuntimeTrust, ERR_UNKNOWN_FAMILY};
use crate::engine::Engine;
use crate::error_key::err;
use crate::runtime::{
    integrity_of, normalize_digest, AssetIntegrity, ReleaseAsset, RuntimeDownloadProgress,
    RuntimeManager, ERR_INTEGRITY_UNVERIFIED,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const ERR_BAD_REPO: &str = "runtime.bad_repo";
pub const ERR_NO_RELEASE: &str = "runtime.no_release";
pub const ERR_GITHUB: &str = "runtime.github_error";
/// whisper.cpp publishes no server build for macOS (only an xcframework).
pub const ERR_WHISPER_NO_MACOS: &str = "runtime.whisper_no_macos";

/// Downloads of extra runtimes share one progress slot and run one at a time.
fn installer() -> &'static RuntimeManager {
    static INSTALLER: OnceLock<RuntimeManager> = OnceLock::new();
    INSTALLER.get_or_init(RuntimeManager::default)
}

/// Progress of the running runtime download (`None` when idle).
pub fn progress() -> Option<RuntimeDownloadProgress> {
    installer().get_download_progress()
}

/// `owner/repo` with GitHub's allowed characters.
pub fn validate_repo(repo: &str) -> Result<(), String> {
    let ok = repo.split('/').count() == 2
        && repo.split('/').all(|p| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        });
    if ok {
        Ok(())
    } else {
        Err(format!("{}: {}", ERR_BAD_REPO, repo))
    }
}

/// What to install from a repository.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReleaseQuery {
    /// Runtime family whose release naming picks the archive (default: `llama.cpp`).
    #[serde(default = "crate::engine::default_family")]
    pub family: String,
    /// `owner/repo` (e.g. `PrismML-Eng/llama.cpp`).
    pub repo: String,
    /// Release tag (default: the newest release that has a matching archive).
    #[serde(default)]
    pub tag: Option<String>,
    /// Exact archive name (default: picked for this OS/arch and `backend`).
    #[serde(default)]
    pub asset: Option<String>,
    /// `vulkan`, `cuda`, `rocm`, `sycl`, `cpu`, `metal` (default: the hardware
    /// recommendation, then Vulkan, then CPU).
    #[serde(default)]
    pub backend: Option<String>,
}

impl Default for ReleaseQuery {
    fn default() -> Self {
        Self {
            family: crate::engine::default_family(),
            repo: String::new(),
            tag: None,
            asset: None,
            backend: None,
        }
    }
}

/// The archive(s) a query resolves to, shown before anything is downloaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReleasePick {
    /// Runtime family the archives belong to.
    pub family: String,
    pub repo: String,
    pub tag: String,
    pub published_at: Option<String>,
    /// Main archive first, then companions (CUDA runtime DLLs).
    #[cfg_attr(feature = "openapi", schema(value_type = Vec<Object>))]
    pub assets: Vec<ReleaseAsset>,
    /// Backend token of the chosen archive (`None` = plain build / explicit asset).
    pub backend: Option<String>,
    /// Names of archives without a usable SHA-256 digest (installing needs confirmation).
    pub unverified: Vec<String>,
}

fn asset_of(a: &serde_json::Value) -> Option<ReleaseAsset> {
    let digest = a.get("digest").and_then(|v| v.as_str()).map(str::to_string);
    Some(ReleaseAsset {
        name: a.get("name")?.as_str()?.to_string(),
        download_url: a.get("browser_download_url")?.as_str()?.to_string(),
        size: a.get("size").and_then(|v| v.as_u64()).unwrap_or(0),
        integrity: integrity_of(digest.as_deref()),
        digest,
    })
}

/// Picks the archive(s) of one release object for a family: an exact `asset` name wins,
/// otherwise the family's own naming scheme picks by `backends`
/// ([`Engine::pick_release_assets`]). Returns `(main + companions, backend)`.
pub fn pick_assets(
    engine: &dyn Engine,
    release: &serde_json::Value,
    asset: Option<&str>,
    backends: &[String],
) -> Option<(Vec<ReleaseAsset>, Option<String>)> {
    let assets: Vec<ReleaseAsset> = release
        .get("assets")?
        .as_array()?
        .iter()
        .filter_map(asset_of)
        .collect();
    if let Some(name) = asset {
        return assets.iter().find(|a| a.name == name).map(|a| {
            let backend = (engine.family() == crate::engine::AUDIO_CPP)
                .then(|| crate::engine::audio_cpp::backend_from_archive(&a.name))
                .flatten();
            (vec![a.clone()], backend)
        });
    }
    engine.pick_release_assets(&assets, backends)
}

/// The engine of a family id; an unknown family is an error, not a llama.cpp guess.
fn family_engine(family: &str) -> Result<&'static dyn Engine, String> {
    crate::engine::engine(family).ok_or_else(|| format!("{}: {}", ERR_UNKNOWN_FAMILY, family))
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("HarnessStudio/0.1.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| crate::http_error::transport(&e, "https://api.github.com"))
}

async fn get_json(url: &str) -> Result<serde_json::Value, String> {
    let res = client()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!("{}: {}", ERR_NO_RELEASE, url));
    }
    if !res.status().is_success() {
        return Err(format!("{}: HTTP {}", ERR_GITHUB, res.status()));
    }
    res.json()
        .await
        .map_err(|e| crate::http_error::transport(&e, url))
}

fn backend_order(requested: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |b: &str| {
        if !out.iter().any(|x| x == b) {
            out.push(b.to_string());
        }
    };
    match requested.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => push(&b.to_ascii_lowercase()),
        None => {
            let hw = crate::hardware::detect_hardware();
            push(hw.recommended_backend.as_str());
            if cfg!(target_os = "macos") {
                push("metal");
            }
            push("vulkan");
            push("cpu");
        }
    }
    out
}

/// Resolves a query against the repository's releases (newest first; releases without a
/// matching archive, like PrismML's cudart-only one, are skipped).
pub async fn find_release(q: &ReleaseQuery) -> Result<ReleasePick, String> {
    let repo = q.repo.trim();
    validate_repo(repo)?;
    let engine = family_engine(&q.family)?;
    let rel = engine.release_source();
    let pinned = (q.family == crate::engine::CRISPASR).then_some(crate::engine::crispasr::VERSION);
    let releases: Vec<serde_json::Value> = match pinned.or(q.tag.as_deref()).map(str::trim) {
        Some(tag) if !tag.is_empty() => {
            if tag.contains('/') || tag.contains("..") {
                return Err(format!("{}: {}", ERR_NO_RELEASE, tag));
            }
            vec![get_json(&rel.release_api_of(repo, tag)).await?]
        }
        _ => get_json(&rel.releases_api_of(repo, 10))
            .await?
            .as_array()
            .cloned()
            .unwrap_or_default(),
    };
    let mut backends = backend_order(q.backend.as_deref());
    if q.family == crate::engine::CRISPASR && q.backend.is_none() && cfg!(target_os = "linux") {
        let hw = crate::hardware::detect_hardware();
        if hw
            .gpus
            .iter()
            .any(|g| g.vendor == crate::hardware::GpuVendor::Amd)
        {
            backends.insert(0, "hip".into());
        }
    }
    for release in releases
        .iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter(|r| engine.accepts_release(r))
    {
        let Some((assets, backend)) = pick_assets(
            engine,
            release,
            q.asset.as_deref().map(str::trim),
            &backends,
        ) else {
            continue;
        };
        let unverified = assets
            .iter()
            .filter(|a| a.integrity != AssetIntegrity::Verified)
            .map(|a| a.name.clone())
            .collect();
        return Ok(ReleasePick {
            family: engine.family().to_string(),
            repo: repo.to_string(),
            tag: release
                .get("tag_name")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string(),
            published_at: release
                .get("published_at")
                .and_then(|t| t.as_str())
                .map(str::to_string),
            assets,
            backend,
            unverified,
        });
    }
    // A search by the family's own naming (no exact asset name) that finds nothing on macOS:
    // say why instead of the generic "no archive", the user's way out is a local build.
    if cfg!(target_os = "macos")
        && engine.family() == crate::engine::WHISPER_CPP
        && q.asset.as_deref().is_none_or(|a| a.trim().is_empty())
    {
        return Err(format!("{}: {}", ERR_WHISPER_NO_MACOS, repo));
    }
    Err(format!(
        "{}: {} ({})",
        ERR_NO_RELEASE,
        repo,
        q.asset.clone().unwrap_or_else(|| backends.join("/"))
    ))
}

/// Install request.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GithubInstall {
    #[serde(flatten)]
    pub query: ReleaseQuery,
    /// Display name (default: `<repo> <tag>`).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    /// The user saw the archive name, size and URL and accepts that it has no digest.
    #[serde(default)]
    pub allow_unverified: bool,
}

/// Downloads, verifies and registers a release. Nothing is downloaded when an archive has
/// no digest and `allow_unverified` is false.
pub async fn install(reg: &RuntimeRegistry, req: &GithubInstall) -> Result<RuntimeEntry, String> {
    let pick = find_release(&req.query).await?;
    if !req.allow_unverified && !pick.unverified.is_empty() {
        return Err(format!(
            "{}: {}",
            ERR_INTEGRITY_UNVERIFIED,
            pick.unverified.join(", ")
        ));
    }
    let engine = family_engine(&pick.family)?;
    let rel = engine.release_source();
    let prefix = rel.download_prefix_of(&pick.repo).to_ascii_lowercase();
    for a in &pick.assets {
        if !a.download_url.to_ascii_lowercase().starts_with(&prefix)
            || a.download_url.contains("..")
        {
            return Err(format!("{}: {}", ERR_BAD_REPO, a.download_url));
        }
    }
    let _busy = installer().try_busy()?;
    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} {}", pick.repo, pick.tag));
    let name =
        if engine.family() == crate::engine::CRISPASR && name.eq_ignore_ascii_case("crispasr") {
            "CrispASR".to_string()
        } else {
            name
        };
    let reg2 = reg.clone();
    let (wanted, name2) = (req.id.clone(), name.clone());
    let id = tokio::task::spawn_blocking(move || reg2.unique_id(wanted.as_deref(), &name2))
        .await
        .map_err(join_failed)??;

    let root = reg.root().to_path_buf();
    let staging = root.join(format!(".staging-{}", id));
    let downloads = root.join(".downloads");
    let _ = tokio::fs::remove_dir_all(&staging).await;
    for dir in [&staging, &downloads] {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }

    let store = reg.shared_store();
    let staged = async {
        for a in &pick.assets {
            let archive = downloads.join(&a.name);
            let digest = a
                .digest
                .as_deref()
                .filter(|d| normalize_digest(d).is_some());
            // The CUDA runtime archive another build already brought: no download.
            let shared = digest
                .and_then(normalize_digest)
                .filter(|_| crate::shared_libs::is_companion(&a.name));
            if let Some(key) = &shared {
                let (s, k, d) = (store.clone(), key.clone(), staging.clone());
                if tokio::task::spawn_blocking(move || s.restore(&k, &d))
                    .await
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            installer()
                .download_to_file(&a.download_url, &archive, &a.name, digest)
                .await?;
            let (arc, dest, s) = (archive.clone(), staging.clone(), store.clone());
            let extracted = tokio::task::spawn_blocking(move || match shared {
                Some(key) if s.add(&key, &arc, &dest).is_ok() => Ok(()),
                _ => crate::runtime::extract_archive(&arc, &dest),
            })
            .await
            .map_err(join_failed)?;
            let _ = tokio::fs::remove_file(&archive).await;
            extracted?;
        }
        let st = staging.clone();
        let binary = rel.binary_file();
        let version_arg = engine.version_arg();
        tokio::task::spawn_blocking(move || {
            crate::runtime::flatten_to_binary_dir_for(&st, &binary)?;
            // Stage 1: the downloaded build must run before it is registered.
            super::probe_version_with(&st.join(&binary), version_arg).map(|_| ())
        })
        .await
        .map_err(join_failed)?
    }
    .await;
    installer().clear_download_progress();
    if let Err(e) = staged {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        let _ = tokio::task::spawn_blocking(move || store.prune()).await;
        return Err(e);
    }

    let dest = root.join(&id);
    let reg2 = reg.clone();
    let trust = if pick.unverified.is_empty() {
        RuntimeTrust::Verified
    } else {
        RuntimeTrust::UserConfirmed
    };
    let tag = pick.tag.clone();
    let source = RuntimeSource::Github {
        repo: pick.repo.clone(),
        tag: pick.tag.clone(),
        assets: pick
            .assets
            .iter()
            .map(|a| super::InstalledAsset {
                name: a.name.clone(),
                url: a.download_url.clone(),
                size: a.size,
                sha256: a.digest.as_deref().and_then(normalize_digest),
            })
            .collect(),
    };
    tokio::task::spawn_blocking(move || {
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| {
                String::from(
                    err("fs.remove_failed")
                        .with("path", dest.display())
                        .with("error", e),
                )
            })?;
        }
        crate::paths::rename_retrying(&staging, &dest).map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            String::from(
                err("fs.rename_failed")
                    .with("from", staging.display())
                    .with("to", dest.display())
                    .with("error", e),
            )
        })?;
        let result = (|| {
            let probe = super::probe_dir(&dest, engine)?;
            if let Some(backend) = probe.backend.as_deref() {
                crate::backend_support::verify_backend_devices(
                    &dest.join(rel.binary_file()),
                    backend,
                )?;
            }
            // A binary that prints no version (whisper-server) gets the release tag's.
            let (version, build) = match probe.version {
                Some(v) => (Some(v), probe.build),
                None => super::version_from_tag(&tag),
            };
            let now = super::now_secs();
            let entry = RuntimeEntry {
                id: id.clone(),
                name,
                family: engine.family().to_string(),
                source,
                dir: dest.clone(),
                version,
                build,
                commit: probe.commit,
                backend: pick.backend.or(probe.backend),
                trust,
                files: super::hash_manifest(&dest)?,
                quant_types: probe.quant_types,
                added_at: now,
                verified_at: now,
            };
            reg2.save_entry(entry.clone())?;
            Ok(entry)
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dest);
        }
        // A replaced build of the same id may have held the last link of a library.
        reg2.shared_store().prune();
        result
    })
    .await
    .map_err(join_failed)?
}

fn join_failed(e: tokio::task::JoinError) -> String {
    err("task.join_failed").with("error", e).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn llama() -> &'static dyn Engine {
        crate::engine::engine(crate::engine::LLAMA_CPP).unwrap()
    }

    /// Asset names of PrismML-Eng/llama.cpp `prism-b10743-adfffbe` (GitHub API, 2026-09-30).
    fn prism_release() -> serde_json::Value {
        let names = [
            (
                "llama-prism-b10743-adfffbe-bin-win-vulkan-x64.zip",
                31302954u64,
                "sha256:d66e0c4d11ea937c8cb197d6c59ffc8c41599bd30ea58057e7e6a4d5a3ced466",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-win-cpu-x64.zip",
                19441568,
                "sha256:d0b3016c9cc4bc1385de68be034adee570277ba952dd94292ba3888b7f18cc44",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-win-cuda-12.4-x64.zip",
                257322810,
                "sha256:1b849f713bee42fda258de83770cd422e8f48dd631ce370eb0641f6458c69d87",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-win-cuda-13.3-x64.zip",
                148892246,
                "sha256:c4f937e2a873180db5cceafca08fae73b5fa0b3fbbf020aeb555c2efe424c7bb",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-win-hip-radeon-x64.zip",
                328355953,
                "sha256:457ea7ed54a11f4ec68180c637fffa120506545b4201a6e7e5343adbb9796d15",
            ),
            (
                "cudart-llama-bin-win-cuda-12.4-x64.zip",
                391443627,
                "sha256:8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-ubuntu-x64.tar.gz",
                17779725,
                "sha256:1bb340929fddae8667c97ec6d4064a5768fe1dd08eeee0074b8d5e08f07dfc31",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-ubuntu-vulkan-x64.tar.gz",
                35914098,
                "sha256:95a3d082629643642842bc2195e0730880273005171517cf857804702f693edc",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-ubuntu-rocm-7.2-x64.tar.gz",
                140087662,
                "sha256:fc539098d17f3a25668027a8f4cd35c7e5b78148abe2cd7948c66ebb988cc203",
            ),
            (
                "llama-prism-b10743-adfffbe-bin-macos-arm64.tar.gz",
                11517506,
                "sha256:596d257973080ca5011a4be50477c5f93ed1d231fcccfc2afb44bb573bb9629a",
            ),
        ];
        serde_json::json!({
            "tag_name": "prism-b10743-adfffbe",
            "assets": names.iter().map(|(n, s, d)| serde_json::json!({
                "name": n,
                "size": s,
                "digest": d,
                "browser_download_url": format!("https://github.com/PrismML-Eng/llama.cpp/releases/download/prism-b10743-adfffbe/{}", n),
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn picks_fork_archives_by_backend() {
        let r = prism_release();
        if cfg!(target_os = "macos") {
            // The fork ships no Vulkan build for macOS; Metal is the plain `macos-arm64` one.
            assert!(pick_assets(llama(), &r, None, &["vulkan".into()]).is_none());
            let (a, b) =
                pick_assets(llama(), &r, None, &["vulkan".into(), "metal".into()]).unwrap();
            assert_eq!(b.as_deref(), Some("metal"));
            assert_eq!(
                a[0].name,
                "llama-prism-b10743-adfffbe-bin-macos-arm64.tar.gz"
            );
        } else {
            let (_, b) = pick_assets(llama(), &r, None, &["vulkan".into()]).unwrap();
            assert_eq!(b.as_deref(), Some("vulkan"));
        }
        if cfg!(windows) {
            let (a, _) = pick_assets(llama(), &r, None, &["vulkan".into()]).unwrap();
            assert_eq!(
                a[0].name,
                "llama-prism-b10743-adfffbe-bin-win-vulkan-x64.zip"
            );
            assert_eq!(a[0].integrity, AssetIntegrity::Verified);
            // ROCm is the fork's `hip-radeon` build on Windows.
            let (a, _) = pick_assets(llama(), &r, None, &["rocm".into()]).unwrap();
            assert_eq!(
                a[0].name,
                "llama-prism-b10743-adfffbe-bin-win-hip-radeon-x64.zip"
            );
            // CUDA: lowest version plus its runtime DLLs.
            let (a, _) = pick_assets(llama(), &r, None, &["cuda".into()]).unwrap();
            assert_eq!(a.len(), 2);
            assert!(a[0].name.contains("cuda-12.4"));
            assert!(a[1].name.starts_with("cudart-"));
            let (a, _) = pick_assets(llama(), &r, None, &["cpu".into()]).unwrap();
            assert!(a[0].name.ends_with("win-cpu-x64.zip"));
        }
        // Explicit name wins; unknown name matches nothing.
        let (a, b) = pick_assets(
            llama(),
            &r,
            Some("cudart-llama-bin-win-cuda-12.4-x64.zip"),
            &[],
        )
        .unwrap();
        assert_eq!(a.len(), 1);
        assert!(b.is_none());
        assert!(pick_assets(llama(), &r, Some("nope.zip"), &[]).is_none());
        // A release without assets for this platform (PrismML's cudart-only release).
        let cudart_only = serde_json::json!({"tag_name": "prism-b10687-5d80cff", "assets": []});
        assert!(pick_assets(
            llama(),
            &cudart_only,
            None,
            &["vulkan".into(), "cpu".into()]
        )
        .is_none());
    }

    /// The family decides which archives match, and a family this build does not know is
    /// refused before any network call (not treated as llama.cpp).
    #[tokio::test]
    async fn family_picks_the_scheme_and_unknown_families_are_refused() {
        let q = ReleaseQuery {
            family: "future-engine.cpp".into(),
            repo: "example/future-engine.cpp".into(),
            ..Default::default()
        };
        let e = find_release(&q).await.unwrap_err();
        assert!(e.starts_with(ERR_UNKNOWN_FAMILY), "{e}");
        assert_eq!(ReleaseQuery::default().family, crate::engine::LLAMA_CPP);
        // The release URLs come from the family's source, for the fork's repo.
        let rel = llama().release_source();
        assert_eq!(
            rel.release_api_of("PrismML-Eng/llama.cpp", "prism-b1"),
            "https://api.github.com/repos/PrismML-Eng/llama.cpp/releases/tags/prism-b1"
        );
        assert_eq!(
            rel.download_prefix_of("PrismML-Eng/llama.cpp"),
            "https://github.com/PrismML-Eng/llama.cpp/releases/download/"
        );
        assert_eq!(rel.download_prefix(), rel.download_prefix_of(rel.repo));
    }

    #[test]
    fn repo_names() {
        assert!(validate_repo("PrismML-Eng/llama.cpp").is_ok());
        assert!(validate_repo("ggml-org/llama.cpp").is_ok());
        for bad in [
            "",
            "a",
            "a/b/c",
            "../x",
            "a/..",
            "a b/c",
            "https://github.com/a/b",
        ] {
            assert!(validate_repo(bad).is_err(), "{bad}");
        }
    }
}
