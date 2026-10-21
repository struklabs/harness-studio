//! Experimental, preset-only offline speech synthesis for CrispASR v0.8.43.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub fn backend(path: &Path) -> Option<&'static str> {
    match crate::models::cached_metadata(path)?
        .architecture
        .as_deref()?
    {
        "kokoro" => Some("kokoro"),
        "pocket-tts"
            if path
                .file_name()
                .is_some_and(|n| is_english_pocket(&n.to_string_lossy())) =>
        {
            Some("pocket-tts")
        }
        _ => None,
    }
}

pub fn is_model(path: &Path) -> bool {
    backend(path).is_some()
}

/// Prefix of the one-request voice files the gateway writes for a cloned voice; anything with it
/// left in the voice folder (a crash) is removed at the next launch.
pub const CLONE_PREFIX: &str = "clone-";

/// Whether the model can speak in a voice cloned from a reference recording: Pocket TTS builds
/// that embed the Mimi encoder (the `novc` builds are decoder only).
pub fn supports_cloning(path: &Path) -> bool {
    backend(path) == Some("pocket-tts")
        && path
            .file_name()
            .is_some_and(|n| !n.to_string_lossy().to_ascii_lowercase().contains("novc"))
}

/// Whether a Hub repo's Pocket TTS files include a cloning build (the repo is tagged
/// `voice-cloning`; its `novc` files do not clone).
pub fn repo_clones(repo: &str, files: &[String]) -> bool {
    repo.eq_ignore_ascii_case("cstr/pocket-tts-GGUF")
        && files.iter().any(|f| {
            let f = f.to_ascii_lowercase();
            f.ends_with(".gguf") && is_english_pocket(&f) && !f.contains("novc")
        })
}

/// Known speech containers must never fall through to a language-model runtime.
pub fn is_tts_arch(arch: &str) -> bool {
    arch.contains("tts")
        || matches!(
            arch,
            "kokoro"
                | "chatterbox"
                | "chatterbox-turbo"
                | "orpheus"
                | "omnivoice"
                | "supertonic"
                | "piper"
                | "speecht5"
                | "zonos"
                | "fish-speech"
        )
}

pub fn is_tts_gguf(path: &Path) -> bool {
    crate::models::cached_metadata(path)
        .and_then(|m| m.architecture)
        .is_some_and(|a| is_tts_arch(&a))
}

/// The bundled Pocket TTS voice is English: other languages in the repo come out unintelligible
/// with it, so only the English builds (with or without the cloning encoder) are offered.
pub fn supports_variant(repo: &str, variant: &str) -> bool {
    !repo.eq_ignore_ascii_case("cstr/pocket-tts-GGUF") || is_english_pocket(variant)
}

fn is_english_pocket(name: &str) -> bool {
    name.to_ascii_lowercase().contains("pocket-tts-english")
}

pub fn supports_repo(repo: &str, task: Option<&str>) -> bool {
    task == Some("text-to-speech")
        && matches!(
            repo.to_ascii_lowercase().as_str(),
            "cstr/kokoro-82m-gguf" | "cstr/pocket-tts-gguf"
        )
}

pub fn required_helpers(repo: &str, variant: &str) -> Vec<super::crispasr_helpers::Helper> {
    use super::crispasr_helpers::{CMUDICT, KOKORO_VOICE, MISAKI, POCKET_VOICE};
    match repo.to_ascii_lowercase().as_str() {
        "cstr/kokoro-82m-gguf" => vec![KOKORO_VOICE, MISAKI, CMUDICT],
        "cstr/pocket-tts-gguf" if is_english_pocket(variant) => vec![POCKET_VOICE],
        _ => Vec::new(),
    }
}

pub fn voice_dir(path: &Path) -> PathBuf {
    let id = Sha256::digest(path.to_string_lossy().as_bytes());
    crate::paths::data_root()
        .join("voices")
        .join("crispasr")
        .join(id.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

pub fn voices(path: &Path) -> Vec<String> {
    use super::crispasr_helpers::{KOKORO_VOICE, POCKET_VOICE};
    let preset = match backend(path) {
        Some("kokoro") if KOKORO_VOICE.verified().is_some() => "af_heart",
        Some("pocket-tts") if POCKET_VOICE.verified().is_some() => "alba",
        _ => return Vec::new(),
    };
    vec![preset.into()]
}

/// Only verified files enter the worker's per-model folder. Never scan arbitrary model siblings.
pub fn launch_args(path: &Path) -> Result<Vec<String>, String> {
    use super::crispasr_helpers::{CMUDICT, KOKORO_VOICE, MISAKI, POCKET_VOICE};
    let backend = backend(path).ok_or("Unsupported CrispASR TTS backend")?;
    let helper = if backend == "kokoro" {
        KOKORO_VOICE
    } else {
        POCKET_VOICE
    };
    let source = helper.verified().ok_or_else(|| {
        String::from(crate::error_key::err("tts.crispasr_helper_missing").with("helper", helper.id))
    })?;
    for dictionary in [MISAKI, CMUDICT] {
        if backend == "kokoro" && dictionary.verified().is_none() {
            return Err(crate::error_key::err("tts.crispasr_helper_missing")
                .with("helper", dictionary.id)
                .into());
        }
    }
    let dir = voice_dir(path);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Reference voices of an interrupted request never outlive the next launch.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with(CLONE_PREFIX)
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let name = if backend == "kokoro" {
        "af_heart.gguf"
    } else {
        "alba.safetensors"
    };
    let target = dir.join(name);
    // Replace rather than follow a pre-existing link in owned storage.
    if target.exists() || target.is_symlink() {
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
    }
    std::fs::copy(source, &target).map_err(|e| e.to_string())?;
    Ok(managed_args(backend, &dir, &target))
}

fn managed_args(backend: &str, dir: &Path, target: &Path) -> Vec<String> {
    vec![
        "--backend".into(),
        backend.into(),
        "--voice-dir".into(),
        dir.to_string_lossy().into(),
        "--voice".into(),
        target.to_string_lossy().into(),
        "--tts-max-input-chars".into(),
        "16000".into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_helpers_cover_only_supported_tts_downloads() {
        let ids = |repo, variant| {
            required_helpers(repo, variant)
                .iter()
                .map(|h| h.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids("cstr/kokoro-82m-GGUF", "kokoro-q8_0"),
            ["kokoro_voice", "misaki", "cmudict"]
        );
        assert_eq!(
            ids("cstr/pocket-tts-GGUF", "pocket-tts-english-q8_0"),
            ["pocket_voice"]
        );
        assert!(ids("cstr/pocket-tts-GGUF", "pocket-tts-german-q8_0").is_empty());
        assert!(ids("other/kokoro", "kokoro-q8_0").is_empty());
    }

    #[test]
    fn only_cloning_builds_of_the_pocket_repo_clone() {
        let files = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert!(repo_clones(
            "cstr/pocket-tts-GGUF",
            &files(&[
                "pocket-tts-english-q8_0.gguf",
                "pocket-tts-english-novc-q8_0.gguf"
            ])
        ));
        assert!(!repo_clones(
            "cstr/pocket-tts-GGUF",
            &files(&["pocket-tts-english-novc-q4_k.gguf"])
        ));
        assert!(!repo_clones(
            "cstr/pocket-tts-GGUF",
            &files(&["pocket-tts-german-q8_0.gguf"])
        ));
        assert!(!repo_clones(
            "cstr/kokoro-82M-GGUF",
            &files(&["kokoro-q8_0.gguf"])
        ));
        assert!(!supports_cloning(Path::new(
            "missing/pocket-tts-english-q8_0.gguf"
        )));
    }

    #[test]
    fn stale_clone_voices_are_removed_at_launch() {
        assert!("clone-abc.wav".starts_with(CLONE_PREFIX));
    }

    #[test]
    fn only_english_pocket_builds_are_offered() {
        let repo = "cstr/pocket-tts-GGUF";
        assert!(supports_variant(repo, "pocket-tts-english-q8_0"));
        assert!(supports_variant(repo, "pocket-tts-english-novc-q4_k"));
        assert!(!supports_variant(repo, "pocket-tts-german-q8_0"));
        assert!(!supports_variant(repo, "pocket-tts-french_24l-f16"));
        assert!(supports_variant("cstr/kokoro-82m-GGUF", "kokoro-82m-q8_0"));
    }
    #[test]
    fn crispasr_tts_owned_voice_folder_and_repo_boundary() {
        assert!(voice_dir(Path::new("model.gguf")).starts_with(crate::paths::data_root()));
        assert_ne!(
            voice_dir(Path::new("a.gguf")),
            voice_dir(Path::new("b.gguf"))
        );
        assert!(supports_repo(
            "cstr/kokoro-82m-GGUF",
            Some("text-to-speech")
        ));
        assert!(!supports_repo(
            "cstr/Qwen3-TTS-GGUF",
            Some("text-to-speech")
        ));
    }

    #[test]
    fn crispasr_tts_launch_arguments_have_owned_paths_and_no_key() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("voices");
        let voice = dir.join("af_heart.gguf");
        let mut args =
            super::super::crispasr_helpers::launch_args(&root.path().join("cache"), None, None);
        args.extend(managed_args("kokoro", &dir, &voice));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "--voice-dir" && w[1] == dir.to_string_lossy()));
        assert!(args.contains(&"--cache-dir".into()));
        assert!(!args
            .iter()
            .any(|arg| arg.contains("key") || arg.contains("secret")));
        assert!(args.contains(&"--no-punctuation".into()));
    }

    /// Set CRISPASR_TEST_BIN and CRISPASR_TEST_MODEL. Companion downloads go through the
    /// pinned queue into an isolated data root before the offline worker is started.
    #[tokio::test]
    #[ignore]
    async fn crispasr_tts_real_offline_worker() {
        use super::super::{crispasr::CrispAsrEngine, ServerEngine};
        use crate::process::{LaunchConfig, ServerCaps};
        use std::process::{Command, Stdio};
        let bin = std::env::var_os("CRISPASR_TEST_BIN").expect("CRISPASR_TEST_BIN");
        let model =
            PathBuf::from(std::env::var_os("CRISPASR_TEST_MODEL").expect("CRISPASR_TEST_MODEL"));
        let data = tempfile::tempdir().unwrap();
        crate::paths::init(data.path().to_path_buf());
        crate::secrets::use_memory_store();
        let manager = crate::downloader::DownloadManager::default();
        let helpers = if backend(&model) == Some("kokoro") {
            vec![
                super::super::crispasr_helpers::KOKORO_VOICE,
                super::super::crispasr_helpers::MISAKI,
                super::super::crispasr_helpers::CMUDICT,
            ]
        } else {
            vec![super::super::crispasr_helpers::POCKET_VOICE]
        };
        for helper in helpers {
            let id = super::super::crispasr_helpers::start_download(&manager, helper.id).unwrap();
            for _ in 0..600 {
                if helper.verified().is_some() {
                    break;
                }
                if let Some(progress) = manager.get(id) {
                    assert!(
                        !matches!(progress.status, crate::downloader::DownloadStatus::Error(_)),
                        "{:?}",
                        progress.status
                    );
                }
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            assert!(
                helper.verified().is_some(),
                "companion {} did not arrive",
                helper.id
            );
        }
        let port = (18610..=18619)
            .find(|p| std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
            .expect("test port");
        let config = LaunchConfig {
            model_path: model,
            host: "127.0.0.1".into(),
            port,
            api_key: Some("test-private-key".into()),
            ..Default::default()
        };
        let args = CrispAsrEngine
            .build_launch(&config, &ServerCaps::default(), None)
            .unwrap()
            .args;
        assert!(args.contains(&"--voice-dir".into()));
        assert!(args.contains(&"--cache-dir".into()));
        assert!(!args.iter().any(|a| a.contains("test-private-key")));
        let home = data.path().join("home");
        std::fs::create_dir(&home).unwrap();
        struct Worker(std::process::Child);
        impl Drop for Worker {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut cmd = Command::new(bin);
        cmd.args(args)
            .envs(CrispAsrEngine.worker_env("test-private-key").unwrap())
            .current_dir(data.path())
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("APPDATA", &home)
            .env("LOCALAPPDATA", &home)
            .env("XDG_CACHE_HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut worker = Worker(cmd.spawn().unwrap());
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap();
        let base = format!("http://127.0.0.1:{port}");
        for _ in 0..300 {
            if http
                .get(format!("{base}/health"))
                .send()
                .await
                .is_ok_and(|r| r.status().is_success())
            {
                break;
            }
            assert!(worker.0.try_wait().unwrap().is_none(), "worker exited");
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let mut body = serde_json::json!({"input":"Hello, this is a local speech test.","response_format":"wav"});
        if backend(&config.model_path) == Some("pocket-tts") {
            body["voice"] = serde_json::json!("alba");
        }
        let res = http
            .post(format!("{base}/v1/audio/speech"))
            .bearer_auth("test-private-key")
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.bytes().await.unwrap();
        assert!(
            status.is_success(),
            "{status}: {}",
            String::from_utf8_lossy(&bytes)
        );
        assert!(crate::audio::wav_duration_secs(&bytes).is_some_and(|s| s > 0.0 && s <= 120.0));
        drop(worker);
        let cache = super::super::crispasr_helpers::cache_dir();
        assert!(
            std::fs::read_dir(cache).unwrap().next().is_none(),
            "worker downloaded a cache file"
        );
        fn unexpected(path: &Path) -> bool {
            std::fs::read_dir(path).unwrap().flatten().any(|e| {
                if e.path().is_dir() {
                    let name = e.file_name();
                    !matches!(
                        name.to_str(),
                        Some("VkCache" | "GLCache" | "DxCache" | "mesa_shader_cache")
                    ) && unexpected(&e.path())
                } else {
                    true
                }
            })
        }
        assert!(
            !unexpected(&home),
            "worker wrote outside owned cache/voice storage"
        );
    }
}
