//! The speech-recognition models Harness offers to download, and the install.
//!
//! The list is curated, not scraped: every entry is an archive of the `asr-models` release of
//! k2-fsa/sherpa-onnx whose licence allows use and redistribution here, with the SHA-256 GitHub
//! publishes for the asset (the download is refused when it does not match). Whisper is not on
//! the list on purpose: whisper.cpp runs it natively (Hub, "whisper" search) and faster. What is
//! offered here is what whisper.cpp cannot run.
//!
//! Turkish: the `asr-models` release has no model trained for Turkish alone. Meta's Omnilingual
//! ASR (1600 languages, Apache-2.0) understands it and is the one non-Whisper choice; its
//! output has no punctuation or capitals.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::{AsrFamily, AsrModel, MODELS_SUBDIR};
use crate::error_key::err;
use crate::runtime::{RuntimeDownloadProgress, RuntimeManager};

/// Where the model archives come from.
pub const RELEASE_DOWNLOAD_PREFIX: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/";

/// Silero VAD (MIT), the 0.6 MB model that cuts audio at pauses.
pub const VAD_URL: &str =
    "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
pub const VAD_SHA256: &str = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";
pub const VAD_SIZE: u64 = 643_854;
const VAD_DIR: &str = ".vad";
const VAD_FILE: &str = "silero_vad.onnx";

/// One downloadable model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CatalogEntry {
    /// Folder name after the install, and what `harness asr install` takes.
    pub id: String,
    /// Display name.
    pub title: String,
    pub family: AsrFamily,
    /// Language codes (`en`, `tr`).
    pub languages: Vec<String>,
    /// Archive size in bytes (the download).
    pub size_bytes: u64,
    /// Licence of the model.
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
    family: AsrFamily,
    languages: &'static [&'static str],
    size: u64,
    license: &'static str,
    sha256: &'static str,
    recommended: bool,
}

const ROWS: &[Row] = &[
    Row {
        id: "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        title: "NVIDIA Parakeet TDT 0.6B v3 (int8, 25 European languages)",
        family: AsrFamily::NemoTransducer,
        languages: &[
            "en", "de", "fr", "es", "it", "pt", "nl", "pl", "ru", "uk", "cs", "sk", "sl", "hr",
            "bg", "ro", "hu", "el", "fi", "sv", "da", "et", "lv", "lt", "mt",
        ],
        size: 487_170_055,
        license: "CC-BY-4.0 (NVIDIA)",
        sha256: "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
        recommended: true,
    },
    Row {
        id: "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17",
        title: "SenseVoice Small (int8: Chinese, English, Japanese, Korean, Cantonese)",
        family: AsrFamily::SenseVoice,
        languages: &["zh", "en", "ja", "ko", "yue"],
        size: 163_002_883,
        license: "FunASR Model Open Source License 1.1 (attribution)",
        sha256: "7d1efa2138a65b0b488df37f8b89e3d91a60676e416f515b952358d83dfd347e",
        recommended: false,
    },
    Row {
        id: "sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-v2-int8-2026-02-05",
        title: "Omnilingual ASR 300M v2 (int8, 1600 languages incl. Turkish)",
        family: AsrFamily::Omnilingual,
        languages: &[
            "tr", "en", "de", "fr", "es", "it", "pt", "ru", "ar", "fa", "hi", "zh", "ja", "ko",
        ],
        size: 292_313_120,
        license: "Apache-2.0 (Meta)",
        sha256: "951b32409aade32bd525310bb39e9666773ba3fc611a39e817f620936d76c631",
        recommended: true,
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

/// Where Harness puts the models it downloads: `<models dir>/asr`.
pub fn models_home() -> PathBuf {
    crate::config::AppSettings::load()
        .primary_models_dir()
        .join(MODELS_SUBDIR)
}

/// Where the VAD model of `home` lives.
pub fn vad_path(home: &Path) -> PathBuf {
    home.join(VAD_DIR).join(VAD_FILE)
}

/// The VAD model of `home`, downloaded (644 KB, checked against its SHA-256) when it is not
/// there yet.
pub async fn ensure_vad_in(home: &Path) -> Result<PathBuf, String> {
    let path = vad_path(home);
    if std::fs::metadata(&path).is_ok_and(|m| m.len() == VAD_SIZE) {
        return Ok(path);
    }
    let _busy = downloader().try_busy()?;
    let dir = home.join(VAD_DIR);
    tokio::fs::create_dir_all(&dir).await.map_err(|e| {
        String::from(
            err("fs.create_dir_failed")
                .with("path", dir.display())
                .with("error", e),
        )
    })?;
    let result = downloader()
        .download_to_file(VAD_URL, &path, VAD_FILE, Some(VAD_SHA256))
        .await;
    downloader().clear_download_progress();
    result?;
    Ok(path)
}

/// [`ensure_vad_in`] the models folder of the settings.
pub async fn ensure_vad() -> Result<PathBuf, String> {
    ensure_vad_in(&models_home()).await
}

/// Downloads a catalog model into [`models_home`] (checked against its SHA-256) and returns the
/// installed model; the VAD model comes with it. An installed one is returned as it is.
pub async fn install(id: &str) -> Result<AsrModel, String> {
    install_into(id, &models_home()).await
}

/// [`install`] into `home` (tests use a temporary folder).
pub async fn install_into(id: &str, home: &Path) -> Result<AsrModel, String> {
    let entry =
        find(id).ok_or_else(|| String::from(err("stt.catalog_not_found").with("id", id)))?;
    let model = crate::tts::catalog::install_archive(
        downloader(),
        &crate::tts::catalog::Archive {
            id: &entry.id,
            url: &entry.url,
            sha256: &entry.sha256,
        },
        home,
        super::detect,
        "stt.archive_not_a_model",
    )
    .await?;
    ensure_vad_in(home).await?;
    Ok(model)
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
        assert_eq!(VAD_SHA256.len(), 64);
        assert!(VAD_URL.starts_with(RELEASE_DOWNLOAD_PREFIX));
    }

    #[test]
    fn the_wanted_models_are_on_offer() {
        let parakeet = find("SHERPA-ONNX-NEMO-PARAKEET-TDT-0.6B-V3-INT8").unwrap();
        assert_eq!(parakeet.family, AsrFamily::NemoTransducer);
        assert_eq!(parakeet.license, "CC-BY-4.0 (NVIDIA)");
        assert!(parakeet.languages.iter().any(|l| l == "de"));
        assert!(
            !parakeet.languages.iter().any(|l| l == "tr"),
            "Parakeet v3 does not do Turkish"
        );
        let sense = catalog()
            .into_iter()
            .find(|e| e.family == AsrFamily::SenseVoice)
            .unwrap();
        assert_eq!(sense.languages, ["zh", "en", "ja", "ko", "yue"]);
        assert!(find("nope").is_none());
    }

    #[test]
    fn a_turkish_model_is_recommended() {
        assert!(catalog()
            .iter()
            .any(|e| e.recommended && e.languages.iter().any(|l| l == "tr")));
    }

    #[test]
    fn licences_allow_use_and_none_is_whisper() {
        assert!(catalog().iter().all(|e| !e.license.contains("NC")));
        assert!(catalog().iter().all(|e| !e.id.contains("whisper")));
    }

    #[test]
    fn vad_lives_in_a_hidden_folder_the_scan_skips() {
        let home = Path::new("models/asr");
        let p = vad_path(home);
        assert!(p.ends_with(".vad/silero_vad.onnx") || p.ends_with(r".vad\silero_vad.onnx"));
    }

    #[test]
    fn every_catalog_entry_is_detected_as_its_own_family() {
        // The folder name alone decides the family of single-file models, so a catalog entry
        // that detect() cannot place would install and then not be found.
        for e in catalog() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join(&e.id);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join("tokens.txt"), "<blk> 0\n").unwrap();
            if e.family == AsrFamily::NemoTransducer || e.family == AsrFamily::Transducer {
                for f in ["encoder", "decoder", "joiner"] {
                    std::fs::write(root.join(format!("{f}.int8.onnx")), b"x").unwrap();
                }
            } else {
                std::fs::write(root.join("model.int8.onnx"), b"x").unwrap();
            }
            let m = super::super::detect(&root).unwrap_or_else(|| panic!("{}", e.id));
            assert_eq!(m.family, e.family, "{}", e.id);
            assert_eq!(m.languages, e.languages);
        }
    }
}
