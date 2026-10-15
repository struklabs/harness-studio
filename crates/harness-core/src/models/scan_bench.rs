use super::*;
use std::time::Instant;

pub(super) fn tiny_gguf(path: &Path) {
    tiny_gguf_arch(path, "llama");
}

pub(crate) fn tiny_gguf_arch(path: &Path, arch: &str) {
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

pub(crate) fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
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
                let mut view =
                    crate::library::list_from(models.to_vec(), &[], &dirs, Some(&db), None);
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
                    .iter()
                    .find(|m| m.id.ends_with("model-0.gguf"))
                    .cloned()
            });
        });
    }
}

thread_local! {
    static STEP: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn step(label: &str) {
    STEP.with(|s| {
        if let Some(start) = s.get() {
            eprintln!(
                "  {label}: {:.3} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
            s.set(Some(Instant::now()));
        }
    });
}

/// File-backed WAL state, populated definitions, alias history and projector pairs.
#[test]
#[ignore]
fn library_list_bench() {
    use crate::domain::store::{DefinitionsFile, Domain, DEFINITIONS_SCHEMA};
    use crate::domain::{ModelDefinition, ModelRef};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("models");
    for i in 0..600 {
        let name = if i % 10 == 9 { "mmproj-model" } else { "model" };
        tiny_gguf_arch(
            &root.join(format!(
                "publisher-{}/repo-{}/{name}-{i}.gguf",
                i % 20,
                i / 10
            )),
            if i % 10 == 9 { "clip" } else { "llama" },
        );
    }
    let dirs = vec![root];
    let models = scan_cache::scan(&dirs);
    let defs: Vec<_> = models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let mut d = ModelDefinition::new(
                format!("alias-{i}"),
                ModelRef::Local {
                    path: m.path.clone(),
                },
            );
            d.id = format!("md-{i}");
            d
        })
        .collect();
    let domain = Domain::at(temp.path());
    crate::store::JsonStore::sensitive(temp.path().join(DEFINITIONS_SCHEMA.file))
        .save_versioned(
            &DEFINITIONS_SCHEMA,
            &DefinitionsFile {
                definitions: defs,
                ..Default::default()
            },
        )
        .unwrap();
    let db = crate::state::StateDb::open(&temp.path().join("state.db")).unwrap();
    db.write(|tx| {
        for (i, m) in models.iter().enumerate() {
            tx.execute("INSERT INTO model_state(model_id, favorite, added_at, last_used_at) VALUES (?1, ?2, 123, 456)", rusqlite::params![crate::library::state_key(&m.path), i % 3 == 0])?;
            for j in 0..4 {
                tx.execute("INSERT INTO conversations(id, model_alias, preset_id, created_at, updated_at) VALUES (?1, ?2, ?3, 1, ?4)", rusqlite::params![format!("conv-{i}-{j}"), format!("alias-{i}"), "builtin:chat", j + 1])?;
            }
        }
        Ok(())
    }).unwrap();
    db.read(|c| {
        let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
        assert_eq!(mode, "wal");
        Ok(())
    })
    .unwrap();
    for pass in 1..=2 {
        eprintln!("600 models, 600 states/definitions, 2400 alias uses: pass {pass}");
        // Pairing can outlast the shared cache TTL on the baseline. Prime outside
        // the measured call so both passes explicitly measure a cached scan.
        scan_cache::scan(&dirs);
        let list_start = Instant::now();
        let defs = timed("definitions loading", || {
            domain.definitions_for_models(&models).unwrap()
        });
        timed("settings loading", crate::config::AppSettings::load);
        let models = timed("cached shared scan", || scan_cache::scan(&dirs));
        STEP.with(|s| s.set(Some(Instant::now())));
        let mut view = timed("library::list_from TOTAL", || {
            crate::library::list_from(models.to_vec(), &defs, &dirs, Some(&db), None)
        });
        STEP.with(|s| s.set(None));
        let installed = timed(
            "installed_families",
            crate::model_registry::installed_families,
        );
        timed("resolve_installed loop", || {
            for e in &mut view.entries {
                e.record.resolve_installed(&installed);
            }
        });
        eprintln!(
            "list_library equivalent TOTAL: {:.3} ms",
            list_start.elapsed().as_secs_f64() * 1000.0
        );
        timed("duplicates grouping (separate command)", || {
            crate::library::dupes::find_duplicates(&models)
        });
    }
}
