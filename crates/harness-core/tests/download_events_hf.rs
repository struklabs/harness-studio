//! Run alone: cargo test -p harness-core --test download_events_hf -- --ignored --nocapture
//! A disposable data root; never uses the installed application's queue or model library.
use harness_core::{
    downloader::{DownloadManager, DownloadStatus, HfModelVariant, HfShardInfo},
    events::{self, CoreEvent},
    paths,
};
use std::time::Duration;

#[tokio::test]
#[ignore = "real Hugging Face download (~19 MB), requires network"]
async fn real_hf_download_events_complete_and_cancel() {
    let data = tempfile::tempdir().unwrap();
    paths::init(data.path().to_path_buf());
    let filename = "tinyllamas/stories15M-q4_0.gguf";
    let variant = HfModelVariant {
        name: "stories15M-q4_0".into(),
        quantization: "Q4_0".into(),
        total_size_bytes: 19_077_344,
        is_split: false,
        shards: vec![HfShardInfo {
            filename: filename.into(),
            download_url: format!("https://huggingface.co/ggml-org/models/resolve/main/{filename}"),
            size_bytes: 19_077_344,
            shard_index: 1,
            total_shards: 1,
            sha256: Some("66967fbece6dbe97886593fdbb73589584927e29119ec31f08090732d1861739".into()),
        }],
    };
    let manager = DownloadManager::default();
    let mut rx = events::subscribe(); // Subscribe before queueing: do not miss started events.
    let id = manager
        .start_variant_download("ggml-org/models".into(), variant)
        .unwrap();
    let mut statuses = Vec::new();
    let mut cancelled = false;
    let mut last_bytes = 0;
    let mut progress_events = 0;
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let event = rx.recv().await.expect("event bus closed or lagged");
            let serialized = serde_json::to_value(&event).unwrap();
            let CoreEvent::Download { progress } = event else {
                continue;
            };
            if progress.id != id {
                continue;
            }
            assert_eq!(serialized["type"], "download");
            assert_eq!(progress.repo_id, "ggml-org/models");
            assert!(progress.downloaded_bytes >= last_bytes, "bytes regressed");
            last_bytes = progress.downloaded_bytes;
            eprintln!(
                "download: {:?}, {}/{}",
                progress.status, last_bytes, progress.total_bytes
            );
            statuses.push(progress.status.clone());
            if let DownloadStatus::Error(error) = &progress.status {
                panic!("{error}");
            }
            if progress.status == DownloadStatus::Downloading && last_bytes > 0 {
                progress_events += 1;
                if !cancelled {
                    manager.cancel_download(Some(id));
                    cancelled = true;
                }
            }
            if progress.status == DownloadStatus::Paused {
                assert!(cancelled);
                assert!(
                    last_bytes < progress.total_bytes,
                    "cancellation must interrupt the download"
                );
                manager.resume_download(id).unwrap();
            }
            if progress.status == DownloadStatus::Completed {
                assert_eq!(last_bytes, 19_077_344);
                break;
            }
        }
    })
    .await
    .expect("download event timeout");
    assert_eq!(statuses.first(), Some(&DownloadStatus::Queued));
    assert!(progress_events > 0);
    let paused = statuses
        .iter()
        .position(|s| *s == DownloadStatus::Paused)
        .expect("cancel event");
    assert!(statuses[..paused].contains(&DownloadStatus::Downloading));
    assert!(statuses[paused + 1..].contains(&DownloadStatus::Queued));
    assert!(statuses[paused + 1..].contains(&DownloadStatus::Downloading));
    assert!(statuses.contains(&DownloadStatus::Verifying));
    assert_eq!(statuses.last(), Some(&DownloadStatus::Completed));
    let target = DownloadManager::models_dir()
        .join("ggml-org/models")
        .join(filename);
    assert_eq!(std::fs::metadata(&target).unwrap().len(), 19_077_344);
    assert_eq!(
        harness_core::downloader::sha256_file(&target).unwrap(),
        "66967fbece6dbe97886593fdbb73589584927e29119ec31f08090732d1861739"
    );
    // Completed can precede the worker's final persistence. Wait for this owned worker before
    // the disposable directory is removed, otherwise it could recreate it after cleanup.
    for _ in 0..100 {
        let persisted =
            std::fs::read_to_string(paths::config_dir().join("downloads.json")).unwrap_or_default();
        if persisted.contains("Completed") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    eprintln!(
        "PASS: queued, progress, cancel/paused, resumed, verifying, completed; SHA-256 verified"
    );
}
