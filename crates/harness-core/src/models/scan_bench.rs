use super::*;
use std::time::Instant;

pub(super) fn tiny_gguf(path: &Path) {
    tiny_gguf_arch(path, "llama");
}

pub(super) fn tiny_gguf_arch(path: &Path, arch: &str) {
    fn string(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u64).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    }
    let mut out = b"GGUF".to_vec();
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&5u64.to_le_bytes());
    for (key, val) in [("general.architecture", arch), ("general.name", "Tiny")] {
        string(&mut out, key);
        out.extend_from_slice(&8u32.to_le_bytes());
        string(&mut out, val);
    }
    for (key, val) in [
        (format!("{arch}.context_length"), 4096u32),
        (format!("{arch}.block_count"), 32),
        ("general.file_type".into(), 15),
    ] {
        string(&mut out, &key);
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(&val.to_le_bytes());
    }
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, out).unwrap();
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    eprintln!("{label}: {:.3} ms", start.elapsed().as_secs_f64() * 1000.0);
    result
}

/// Run with `cargo test -p harness-core library_scan_bench -- --ignored --nocapture`.
/// Uses only temporary files and the test process's isolated data root.
#[test]
#[ignore]
fn library_scan_bench() {
    let temp = tempfile::tempdir().unwrap();
    let n = std::env::var("LIBRARY_BENCH_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    for i in 0..n {
        tiny_gguf(
            &temp
                .path()
                .join(format!("publisher-{}/repo-{i}/model-{i}.gguf", i % 20)),
        );
    }
    let dirs = vec![temp.path().to_path_buf()];
    let settings = AppSettings {
        models_dir: temp.path().to_string_lossy().into_owned(),
        ..Default::default()
    };
    let db = crate::state::StateDb::open(&temp.path().join("state.db")).unwrap();
    for pass in ["cold", "warm"] {
        eprintln!("N={n} {pass}");
        METADATA_STATS.with(|s| s.set((0, 0)));
        let mut paths = Vec::new();
        timed("collect", || {
            ModelManager.collect_gguf_files(temp.path(), &mut paths)
        });
        let models = timed("scan_files (GGUF metadata)", || {
            ModelManager.scan_files(temp.path(), paths)
        });
        METADATA_STATS.with(|s| eprintln!("GGUF cache (hits, misses): {:?}", s.get()));
        timed("tts", || crate::tts::library_models(&dirs));
        timed("asr", || crate::asr::library_models(&dirs));
        timed("mistralrs", || {
            crate::engine::mistralrs::library_models(&dirs)
        });
        timed("python_hf", || {
            crate::engine::python_hf::library_models(&dirs)
        });
        timed("mlx", || crate::engine::mlx::library_models(&dirs));
        let tree = timed("shared inventory", || {
            discovery::DirectoryTree::collect(temp.path())
        });
        timed("inventory tts", || {
            crate::tts::library_models_in(temp.path(), &tree)
        });
        timed("inventory asr", || {
            crate::asr::library_models_in(temp.path(), &tree)
        });
        timed("inventory mistralrs", || {
            crate::engine::mistralrs::library_models_in(temp.path(), &tree)
        });
        timed("inventory python_hf", || {
            crate::engine::python_hf::library_models_in(temp.path(), &tree)
        });
        timed("inventory mlx", || {
            crate::engine::mlx::library_models_in(temp.path(), &tree)
        });
        timed("scan_directories", || ModelManager.scan_directories(&dirs));
        timed("arrival metadata", || {
            models
                .iter()
                .map(crate::library::arrival_time)
                .collect::<Vec<_>>()
        });
        timed("local_format", || {
            models
                .iter()
                .map(crate::model_registry::local_format)
                .collect::<Vec<_>>()
        });
        timed("local_record", || {
            models
                .iter()
                .map(|m| crate::model_registry::local_record(m, &Default::default()))
                .collect::<Vec<_>>()
        });
        timed("library::list_from (with DB)", || {
            crate::library::list_from(models.clone(), &[], &dirs, Some(&db), None)
        });
        timed("storage counts + disk stats", || {
            crate::storage::StorageManager::get_storage_directories(&settings, &models)
        });
        timed(
            "installed_families",
            crate::model_registry::installed_families,
        );
    }
    for pass in ["expired", "cached"] {
        if pass == "expired" {
            invalidate_scan();
        }
        eprintln!("page open ({pass})");
        timed("TOTAL page backend (sequential)", || {
            timed("list_library equivalent", || {
                let models = scan_cache::scan(&dirs);
                let mut view = crate::library::list_from(models, &[], &dirs, Some(&db), None);
                let installed = crate::model_registry::installed_families();
                for entry in &mut view.entries {
                    entry.record.resolve_installed(&installed);
                }
                view
            });
            timed("get_storage_directories equivalent", || {
                let models = scan_cache::scan(&dirs);
                crate::storage::StorageManager::get_storage_directories(&settings, &models)
            });
            timed(
                "listAgentModels (empty definitions)",
                crate::integrations::agent_models::list,
            )
            .unwrap();
            timed("scan_local_models equivalent", || scan_cache::scan(&dirs));
            timed("find_model equivalent", || {
                scan_cache::scan(&dirs)
                    .into_iter()
                    .find(|m| m.id.ends_with("model-0.gguf"))
            });
        });
    }
}
