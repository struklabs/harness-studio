//! The text-to-speech models Harness offers to download, and the install.
//!
//! The list is curated, not scraped: every entry is an archive of the `tts-models` release of
//! k2-fsa/sherpa-onnx whose licence allows use and redistribution here, with the SHA-256 GitHub
//! publishes for the asset (the download is refused when it does not match). Piper's *code* is
//! GPL-3.0 and is not part of Harness; the Piper voices run through sherpa-onnx, and only voices
//! trained on data with a free licence are listed (CC0 for the Turkish ones; the `dfki` Turkish
//! voice is CC BY-NC-SA and is left out). Kokoro is Apache-2.0.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::{TtsFamily, TtsModel, MODELS_SUBDIR};
use crate::error_key::err;
use crate::runtime::{RuntimeDownloadProgress, RuntimeManager};

/// Where the model archives come from.
pub const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/";

/// One downloadable model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CatalogEntry {
    /// Folder name after the install, and what `harness tts install` takes.
    pub id: String,
    /// Display name.
    pub title: String,
    pub family: TtsFamily,
    /// Language codes (`en`, `tr`).
    pub languages: Vec<String>,
    /// Voices in the model.
    pub voices: u32,
    /// Archive size in bytes (the download).
    pub size_bytes: u64,
    /// Licence of the model and of the data it was trained on.
    pub license: String,
    /// Where the archive comes from.
    pub url: String,
    /// SHA-256 of the archive.
    pub sha256: String,
    /// Shown first and installed by the one-click setup.
    pub recommended: bool,
}

struct Row {
    id: &'static str,
    title: &'static str,
    family: TtsFamily,
    languages: &'static [&'static str],
    voices: u32,
    size: u64,
    license: &'static str,
    sha256: &'static str,
    recommended: bool,
}

const ROWS: &[Row] = &[
    Row {
        id: "kokoro-int8-multi-lang-v1_0",
        title: "Kokoro 82M v1.0 (int8)",
        family: TtsFamily::Kokoro,
        languages: &["en", "es", "fr", "hi", "it", "ja", "pt", "zh"],
        voices: 54,
        size: 132_303_094,
        license: "Apache-2.0",
        sha256: "4c3052abaa60943a341f193888cf6abd68787dae6ab8ae5c925a706caa247e4e",
        recommended: true,
    },
    Row {
        id: "vits-piper-tr_TR-fettah-medium-int8",
        title: "Piper Turkish, fettah (medium, int8)",
        family: TtsFamily::Vits,
        languages: &["tr"],
        voices: 1,
        size: 21_186_168,
        license: "CC0 (NabuCasa voice datasets)",
        sha256: "4374bfdabb88ada0f3edf9bd46eacf1c5391a8a93bdce364196495891e5bc323",
        recommended: true,
    },
    Row {
        id: "vits-piper-tr_TR-fahrettin-medium-int8",
        title: "Piper Turkish, fahrettin (medium, int8)",
        family: TtsFamily::Vits,
        languages: &["tr"],
        voices: 1,
        size: 21_118_817,
        license: "CC0 (NabuCasa voice datasets)",
        sha256: "9897e4da7f26be2bd03dc920d9c698634060832cbdb18b7d2908728db1465089",
        recommended: false,
    },
];

/// The models on offer.
pub fn catalog() -> Vec<CatalogEntry> {
    ROWS.iter()
        .map(|r| CatalogEntry {
            id: r.id.to_string(),
            title: r.title.to_string(),
            family: r.family,
            languages: r.languages.iter().map(|l| (*l).to_string()).collect(),
            voices: r.voices,
            size_bytes: r.size,
            license: r.license.to_string(),
            url: format!("{RELEASE_DOWNLOAD_PREFIX}{}.tar.bz2", r.id),
            sha256: r.sha256.to_string(),
            recommended: r.recommended,
        })
        .collect()
}

/// One entry by id.
pub fn find(id: &str) -> Option<CatalogEntry> {
    let id = id.trim();
    catalog()
        .into_iter()
        .find(|e| e.id.eq_ignore_ascii_case(id))
}

fn downloader() -> &'static RuntimeManager {
    static DOWNLOADS: OnceLock<RuntimeManager> = OnceLock::new();
    DOWNLOADS.get_or_init(RuntimeManager::default)
}

/// Progress of the running model download (`None` when idle).
pub fn install_progress() -> Option<RuntimeDownloadProgress> {
    downloader().get_download_progress()
}

/// Where Harness puts the models it downloads: `<models dir>/tts`.
pub fn models_home() -> PathBuf {
    crate::config::AppSettings::load()
        .primary_models_dir()
        .join(MODELS_SUBDIR)
}

fn join_failed(e: tokio::task::JoinError) -> String {
    err("task.join_failed").with("error", e).into()
}

/// Downloads a catalog model into [`models_home`] (checked against its SHA-256) and returns the
/// installed model. An installed one is returned as it is.
pub async fn install(id: &str) -> Result<TtsModel, String> {
    install_into(id, &models_home()).await
}

/// [`install`] into `home` (tests use a temporary folder).
pub async fn install_into(id: &str, home: &Path) -> Result<TtsModel, String> {
    let entry =
        find(id).ok_or_else(|| String::from(err("tts.catalog_not_found").with("id", id)))?;
    install_archive(
        downloader(),
        &Archive {
            id: &entry.id,
            url: &entry.url,
            sha256: &entry.sha256,
        },
        home,
        super::detect,
        "tts.archive_not_a_model",
    )
    .await
}

/// A `.tar.bz2` model archive whose single top-level folder is named `id`.
pub(crate) struct Archive<'a> {
    pub id: &'a str,
    pub url: &'a str,
    pub sha256: &'a str,
}

/// Downloads `archive` (checked against its SHA-256), unpacks it into `home/<id>` and returns
/// what `detect` makes of the folder; an installed model is returned as it is. Shared by the
/// text-to-speech and speech-recognition catalogs.
pub(crate) async fn install_archive<T>(
    dl: &RuntimeManager,
    archive: &Archive<'_>,
    home: &Path,
    detect: fn(&Path) -> Option<T>,
    not_a_model_key: &'static str,
) -> Result<T, String> {
    let id = archive.id;
    let target = home.join(id);
    if let Some(model) = detect(&target) {
        return Ok(model);
    }
    let _busy = dl.try_busy()?;
    let downloads = home.join(".downloads");
    let staging = home.join(format!(".staging-{id}"));
    let _ = tokio::fs::remove_dir_all(&staging).await;
    for dir in [&downloads, &staging] {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            String::from(
                err("fs.create_dir_failed")
                    .with("path", dir.display())
                    .with("error", e),
            )
        })?;
    }
    let file = downloads.join(format!("{id}.tar.bz2"));
    let result = async {
        dl.download_to_file(archive.url, &file, id, Some(archive.sha256))
            .await?;
        let (a, s) = (file.clone(), staging.clone());
        tokio::task::spawn_blocking(move || crate::runtime::extract_archive(&a, &s))
            .await
            .map_err(join_failed)??;
        let unpacked = staging.join(id);
        if detect(&unpacked).is_none() {
            return Err(String::from(err(not_a_model_key).with("id", id)));
        }
        let _mutation = crate::models::LibraryMutation::begin();
        if target.exists() {
            let _ = tokio::fs::remove_dir_all(&target).await;
        }
        {
            let (u, t) = (unpacked.clone(), target.clone());
            tokio::task::spawn_blocking(move || crate::paths::rename_retrying(&u, &t))
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))
                .and_then(|r| r)
        }
        .map_err(|e| {
            String::from(
                err("fs.rename_failed")
                    .with("from", unpacked.display())
                    .with("to", target.display())
                    .with("error", e),
            )
        })
    }
    .await;
    dl.clear_download_progress();
    let _ = tokio::fs::remove_file(&file).await;
    let _ = tokio::fs::remove_dir_all(&staging).await;
    result?;
    detect(&target).ok_or_else(|| String::from(err(not_a_model_key).with("id", id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_pinned_and_from_the_official_release() {
        let all = catalog();
        assert!(all.len() >= 3);
        for e in &all {
            assert!(e.url.starts_with(RELEASE_DOWNLOAD_PREFIX), "{}", e.url);
            assert!(e.url.ends_with(&format!("{}.tar.bz2", e.id)));
            assert_eq!(e.sha256.len(), 64, "{}", e.id);
            assert!(e.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(e.size_bytes > 1_000_000);
            assert!(!e.languages.is_empty() && !e.license.is_empty());
        }
        let ids: std::collections::HashSet<_> = all.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "ids are unique");
    }

    #[test]
    fn licences_allow_use() {
        // The Turkish voice trained on non-commercial data must not be on offer.
        assert!(catalog().iter().all(|e| !e.id.contains("dfki")));
        assert!(catalog().iter().all(|e| !e.license.contains("NC")));
        let kokoro = find("KOKORO-int8-multi-lang-v1_0").unwrap();
        assert_eq!(kokoro.license, "Apache-2.0");
        assert_eq!(kokoro.family, TtsFamily::Kokoro);
        assert!(find("nope").is_none());
    }

    #[test]
    fn a_turkish_voice_is_recommended() {
        assert!(catalog()
            .iter()
            .any(|e| e.recommended && e.languages.iter().any(|l| l == "tr")));
    }
}
