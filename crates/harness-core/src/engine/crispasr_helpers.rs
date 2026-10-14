//! CrispASR's helper models (language identification, punctuation restoration).
//!
//! CrispASR downloads these by itself when a request needs them (into `~/.cache/crispasr`, with
//! no hash check). Harness never lets the worker do that: the two files below are pinned to a
//! repository revision with a SHA-256, come through the download queue like every other model,
//! and are handed to the worker only as local paths.
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One pinned helper file.
#[derive(Debug, Clone, Copy)]
pub struct Helper {
    pub id: &'static str,
    pub title: &'static str,
    pub repo: &'static str,
    pub rev: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// Language identification (Whisper tiny), used when a request names no language.
pub const LID: Helper = Helper {
    id: "lid",
    title: "Language Detection",
    repo: "ggerganov/whisper.cpp",
    rev: "5359861c739e955e79d9a303bcbc70fb988958b1",
    file: "ggml-tiny.bin",
    size: 77_691_713,
    sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
};

/// Punctuation restoration for backends whose output has none.
pub const PUNC: Helper = Helper {
    id: "punc",
    title: "Punctuation Restoration",
    repo: "cstr/fireredpunc-GGUF",
    rev: "6d21e1f5c1e2179579d07918362972bf3d069a4b",
    file: "fireredpunc-q4_k.gguf",
    size: 57_886_944,
    sha256: "faf4a43e3135bc307a66194685af00f756e6f4c28c7d9e2dd8f3517cddca5c45",
};

pub const KOKORO_VOICE: Helper = Helper {
    id: "kokoro_voice",
    title: "Kokoro Voice",
    repo: "cstr/kokoro-voices-GGUF",
    rev: "1c98db113c69e3feef9644cf0aaec5c0bea02a8e",
    file: "kokoro-voice-af_heart.gguf",
    size: 522_784,
    sha256: "14463a5a5204a16a382b2da87b4b0206fcda225b44b7c4627999eb527f49c018",
};
pub const POCKET_VOICE: Helper = Helper {
    id: "pocket_voice",
    title: "Pocket TTS Voice",
    // Public prepared voice state (the original kyutai/pocket-tts repo is gated).
    repo: "audio-cpp/audio.cpp-gguf",
    rev: "f861648c0c1f4da9f18e9a0ce5e217b0679bb2f5",
    file: "PocketTTS-GGUF/english/embeddings/alba.safetensors",
    size: 6_194_424,
    sha256: "69c32db63ca56843d994f81f343f62e0bf2d73f7e4c9bc73e44bb1110b1d8845",
};
pub const MISAKI: Helper = Helper {
    id: "misaki",
    title: "Pronunciation Dictionary",
    repo: "hexgrad/misaki",
    rev: "fba1236595f2d2bf21d414ba6e57d25256afada3",
    file: "misaki/data/us_gold.json",
    size: 3_000_469,
    sha256: "dc414872a49a28ae6c141463d502fd945f3b2fde040484fdc47d00cc4612686f",
};
pub const CMUDICT: Helper = Helper {
    id: "cmudict",
    title: "English Fallback Dictionary",
    repo: "cmusphinx/cmudict",
    rev: "74790861f652b15e4ac49015a90074ad62a27690",
    file: "cmudict.dict",
    size: 3_618_488,
    sha256: "81917843c7f44ce2b094ac63873c2c7a4cf802040792c455ba3ca406891c3d22",
};
pub const HELPERS: [Helper; 6] = [LID, PUNC, KOKORO_VOICE, POCKET_VOICE, MISAKI, CMUDICT];

impl Helper {
    /// The pinned download address (a revision, never `main`).
    pub fn url(&self) -> String {
        if matches!(self.id, "misaki" | "cmudict") {
            return format!(
                "https://raw.githubusercontent.com/{}/{}/{}",
                self.repo, self.rev, self.file
            );
        }
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.rev, self.file
        )
    }
    /// Where the file lives under `root`.
    pub fn path_in(&self, root: &Path) -> PathBuf {
        helpers_dir_in(root).join(self.file)
    }
    /// The file when it is complete under `root` (the downloader verified its SHA-256 before
    /// moving it here, so the size is the quick check).
    pub fn installed_in(&self, root: &Path) -> Option<PathBuf> {
        let path = self.path_in(root);
        std::fs::metadata(&path)
            .is_ok_and(|m| m.len() == self.size)
            .then_some(path)
    }
    pub fn installed(&self) -> Option<PathBuf> {
        self.installed_in(&crate::paths::data_root())
    }
    /// Recheck preset/dictionary bytes before they can become a filesystem allowlist.
    pub fn verified(&self) -> Option<PathBuf> {
        use sha2::{Digest, Sha256};
        let path = self.installed()?;
        if std::fs::symlink_metadata(&path)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
        let hash = Sha256::digest(std::fs::read(&path).ok()?);
        (hash.iter().map(|b| format!("{b:02x}")).collect::<String>() == self.sha256).then_some(path)
    }
}

pub fn find(id: &str) -> Option<Helper> {
    HELPERS.into_iter().find(|h| h.id == id)
}

pub fn helpers_dir_in(root: &Path) -> PathBuf {
    root.join("helpers").join("crispasr")
}

pub fn helpers_dir() -> PathBuf {
    helpers_dir_in(&crate::paths::data_root())
}

/// CrispASR's own cache folder: always set, and always empty of anything downloaded.
pub fn cache_dir_in(root: &Path) -> PathBuf {
    root.join("cache").join("crispasr")
}

pub fn cache_dir() -> PathBuf {
    cache_dir_in(&crate::paths::data_root())
}

/// A request that names no language is only answerable when language identification is there.
pub fn language_known(language: Option<&str>) -> bool {
    language
        .map(str::trim)
        .is_some_and(|l| !l.is_empty() && !l.eq_ignore_ascii_case("auto"))
}

/// What Settings shows for one helper.
#[derive(Debug, Clone, Serialize)]
pub struct HelperStatus {
    pub id: &'static str,
    pub title: &'static str,
    pub file: &'static str,
    pub size_bytes: u64,
    pub installed: bool,
}

pub fn status() -> Vec<HelperStatus> {
    HELPERS
        .iter()
        .map(|h| HelperStatus {
            id: h.id,
            title: h.title,
            file: h.file,
            size_bytes: h.size,
            installed: if matches!(h.id, "lid" | "punc") {
                h.installed().is_some()
            } else {
                h.verified().is_some()
            },
        })
        .collect()
}

/// Queues the download of helper `id` (pinned revision, SHA-256 checked) in the download queue.
pub fn start_download(
    manager: &crate::downloader::DownloadManager,
    id: &str,
) -> Result<u64, String> {
    let helper = find(id).ok_or_else(|| format!("Unknown CrispASR helper model: {id}"))?;
    let file = crate::downloader::PinnedFile {
        repo: helper.repo,
        rev: helper.rev,
        filename: helper.file,
        size: helper.size,
        sha256: helper.sha256,
    };
    if matches!(helper.id, "misaki" | "cmudict") {
        return manager.start_pinned_github_download(
            &file,
            format!("{} (CrispASR helper)", helper.file),
            helpers_dir(),
        );
    }
    manager.start_pinned_download(
        &file,
        format!("{} (CrispASR helper)", helper.file),
        helpers_dir(),
    )
}

/// Deletes the file of helper `id`.
pub fn remove(id: &str) -> Result<(), String> {
    let helper = find(id).ok_or_else(|| format!("Unknown CrispASR helper model: {id}"))?;
    let path = helper.path_in(&crate::paths::data_root());
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Launch arguments about the helper models and the cache folder. A missing language
/// identification file turns it off (requests then have to name their language); a missing
/// punctuation file turns punctuation restoration off. The worker is never left to fetch either.
pub fn launch_args(cache: &Path, lid: Option<&Path>, punc: Option<&Path>) -> Vec<String> {
    let mut args = vec!["--cache-dir".into(), cache.to_string_lossy().into_owned()];
    match lid {
        Some(path) => {
            args.push("--lid-model".into());
            args.push(path.to_string_lossy().into_owned());
        }
        None => {
            args.push("--lid-backend".into());
            args.push("off".into());
        }
    }
    match punc {
        Some(path) => {
            args.push("--punc-model".into());
            args.push(path.to_string_lossy().into_owned());
        }
        None => args.push("--no-punctuation".into()),
    }
    args
}

/// Environment that keeps the worker off the network even if it tried: its cache folder is
/// ours, and every proxy variable points at a closed port.
pub fn worker_env(cache: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    const BLACKHOLE: &str = "http://127.0.0.1:9";
    vec![
        ("CRISPASR_CACHE_DIR", cache.as_os_str().to_owned()),
        ("HTTP_PROXY", BLACKHOLE.into()),
        ("HTTPS_PROXY", BLACKHOLE.into()),
        ("ALL_PROXY", BLACKHOLE.into()),
        ("NO_PROXY", "".into()),
        ("http_proxy", BLACKHOLE.into()),
        ("https_proxy", BLACKHOLE.into()),
        ("all_proxy", BLACKHOLE.into()),
        ("no_proxy", "".into()),
    ]
}

/// Which helper models a worker was started with: (language identification, punctuation).
type LaunchNotes = std::sync::Mutex<std::collections::HashMap<u16, (bool, bool)>>;

/// Helper models of each running worker, by port, so a request that names no language can be
/// refused when its worker has no language identification, and a worker started before a
/// helper arrived (or after one was removed) can be restarted.
fn launch_notes() -> &'static LaunchNotes {
    static MAP: std::sync::OnceLock<LaunchNotes> = std::sync::OnceLock::new();
    MAP.get_or_init(Default::default)
}

pub fn note_launch(port: u16, with_lid: bool, with_punc: bool) {
    launch_notes()
        .lock()
        .unwrap()
        .insert(port, (with_lid, with_punc));
}

pub fn worker_has_lid(port: u16) -> bool {
    launch_notes()
        .lock()
        .unwrap()
        .get(&port)
        .is_some_and(|(lid, _)| *lid)
}

/// True when the worker on `port` was started with other helper models than are installed now.
/// Unknown ports (not started by this process) are never outdated.
pub fn worker_outdated(port: u16) -> bool {
    let now = (LID.installed().is_some(), PUNC.installed().is_some());
    launch_notes()
        .lock()
        .unwrap()
        .get(&port)
        .is_some_and(|launched| *launched != now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_are_pinned_to_revisions_with_hashes() {
        for h in HELPERS {
            assert_eq!(h.rev.len(), 40, "{}", h.id);
            assert_eq!(h.sha256.len(), 64);
            assert!(h.url().contains(h.rev));
            assert!(!h.url().contains("/main/"));
        }
        assert_eq!(LID.size, 77_691_713);
        assert_eq!(PUNC.size, 57_886_944);
    }

    #[test]
    fn installed_needs_the_exact_size() {
        let dir = tempfile::tempdir().unwrap();
        assert!(LID.installed_in(dir.path()).is_none());
        let path = LID.path_in(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"short").unwrap();
        assert!(LID.installed_in(dir.path()).is_none());
        assert!(path.starts_with(dir.path().join("helpers").join("crispasr")));
    }

    #[test]
    fn launch_args_use_local_paths_or_turn_the_feature_off() {
        let cache = Path::new("/data/cache/crispasr");
        let both = launch_args(
            cache,
            Some(Path::new("/h/lid.bin")),
            Some(Path::new("/h/p.gguf")),
        );
        assert_eq!(
            both,
            [
                "--cache-dir",
                "/data/cache/crispasr",
                "--lid-model",
                "/h/lid.bin",
                "--punc-model",
                "/h/p.gguf"
            ]
        );
        let none = launch_args(cache, None, None);
        assert_eq!(
            none,
            [
                "--cache-dir",
                "/data/cache/crispasr",
                "--lid-backend",
                "off",
                "--no-punctuation"
            ]
        );
    }

    #[test]
    fn worker_env_has_the_cache_and_a_dead_proxy() {
        let env = worker_env(Path::new("/c"));
        let get = |k: &str| env.iter().find(|e| e.0 == k).map(|e| e.1.clone());
        assert_eq!(get("CRISPASR_CACHE_DIR").unwrap(), "/c");
        for k in [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ] {
            assert_eq!(get(k).unwrap(), "http://127.0.0.1:9", "{k}");
        }
        assert_eq!(get("NO_PROXY").unwrap(), "");
        assert_eq!(get("no_proxy").unwrap(), "");
    }

    #[test]
    fn a_language_is_known_unless_empty_or_auto() {
        assert!(language_known(Some("tr")));
        assert!(!language_known(None));
        assert!(!language_known(Some("  ")));
        assert!(!language_known(Some("AUTO")));
    }
}
