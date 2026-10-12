/// Existing definition files outside the library, using the same GGUF and shard scan rules.
#[tauri::command]
pub(crate) async fn inspect_model_paths(paths: Vec<PathBuf>) -> Result<Vec<LocalModel>, String> {
    blocking(move || ModelManager.inspect_gguf_paths(&paths)).await
}

#[tauri::command]
pub(crate) async fn scan_local_models(
    model_mgr: State<'_, Arc<ModelManager>>,
) -> Result<Vec<LocalModel>, String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || mgr.scan_models()).await
}

#[tauri::command]
pub(crate) async fn get_model_metadata(path: String) -> Result<ModelMetadata, String> {
    blocking(move || {
        let metadata = read_gguf_metadata(Path::new(&path))?;
        Ok(ModelMetadata {
            known_metadata_fixes: metadata.known_metadata_fixes(),
            metadata,
        })
    })
    .await?
}

#[derive(serde::Serialize)]
pub(crate) struct ModelMetadata {
    #[serde(flatten)]
    metadata: GgufMetadata,
    known_metadata_fixes: harness_core::gguf::KnownMetadataFixes,
}

#[tauri::command]
pub(crate) async fn delete_local_model(
    model_mgr: State<'_, Arc<ModelManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
    model_id: String,
) -> Result<(), String> {
    let _model_lease = gate.model_write(Path::new(&model_id)).await;
    ensure_model_not_in_use(&registry, &model_id)?;
    let mgr = model_mgr.inner().clone();
    blocking(move || mgr.delete_model(&model_id)).await?
}

#[tauri::command]
pub(crate) async fn scan_import_sources() -> Result<importer::ImportScan, String> {
    blocking(importer::scan).await
}

#[tauri::command]
pub(crate) async fn import_models(
    paths: Vec<PathBuf>,
    mode: importer::ImportMode,
) -> Result<Vec<importer::ImportResult>, String> {
    blocking(move || importer::import(&paths, mode)).await
}

/// Removes a linked (Ollama / LM Studio) model from the library; its file is untouched.
#[tauri::command]
pub(crate) async fn unlink_external_model(
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
    model_id: String,
) -> Result<(), String> {
    let _model_lease = gate.model_write(Path::new(&model_id)).await;
    ensure_model_not_in_use(&registry, &model_id)?;
    importer::unlink_external(Path::new(&model_id))
}

#[tauri::command]
pub(crate) fn get_model_preset(model_id: String) -> Option<ModelPreset> {
    ModelManager::get_saved_preset(&model_id)
}

#[tauri::command]
pub(crate) fn save_model_preset(model_id: String, preset: ModelPreset) -> Result<(), String> {
    ModelManager::save_preset(&model_id, preset)
}

#[tauri::command]
pub(crate) async fn get_storage_directories(
    model_mgr: State<'_, Arc<ModelManager>>,
) -> Result<Vec<StorageDirInfo>, String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || {
        let settings = AppSettings::load();
        let models = mgr.scan_models();
        StorageManager::get_storage_directories(&settings, &models)
    })
    .await
}

#[tauri::command]
pub(crate) fn add_storage_directory(dir_path: String) -> Result<AppSettings, String> {
    StorageManager::add_storage_directory(&dir_path)
}

#[tauri::command]
pub(crate) fn remove_storage_directory(dir_path: String) -> Result<AppSettings, String> {
    StorageManager::remove_storage_directory(&dir_path)
}

#[tauri::command]
pub(crate) fn set_primary_storage_directory(dir_path: String) -> Result<AppSettings, String> {
    StorageManager::set_primary_storage_directory(&dir_path)
}

#[tauri::command]
pub(crate) async fn migrate_model(
    model_mgr: State<'_, Arc<ModelManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
    model_id: String,
    target_dir: String,
) -> Result<String, String> {
    let _model_lease = gate.model_write(Path::new(&model_id)).await;
    ensure_model_not_in_use(&registry, &model_id)?;
    let mgr = model_mgr.inner().clone();
    blocking(move || {
        let model = mgr.find_model(&model_id).ok_or_else(|| {
            String::from(harness_core::error_key::err("models.not_found").with("id", &model_id))
        })?;
        let new_path = StorageManager::migrate_model(&model, &target_dir)?;
        let new_id = new_path.to_string_lossy().to_string();
        // Presets are keyed by model id (= launch path); carry them over.
        ModelManager::rename_preset(&model_id, &new_id);
        // The download address of each part follows it too (needed to repair the set later).
        if let Some(dir) = new_path.parent() {
            let moved: Vec<(PathBuf, PathBuf)> = model
                .shard_paths
                .iter()
                .filter_map(|old| Some((old.clone(), dir.join(old.file_name()?))))
                .collect();
            let pairs: Vec<(&Path, &Path)> = moved
                .iter()
                .map(|(a, b)| (a.as_path(), b.as_path()))
                .collect();
            harness_core::library::sources::renamed(&pairs);
        }
        // Library state (favorite, last use) follows the file.
        if let Ok(db) = crate::state::shared() {
            let key = |id: &str| harness_core::library::state_key(Path::new(id));
            if let Err(e) = db.rename_model_state(&key(&model_id), &key(&new_id)) {
                eprintln!("[Harness Studio] Kütüphane durumu taşınamadı: {}", e);
            }
        }
        Ok(new_id)
    })
    .await?
}

#[cfg(test)]
mod definition_path_tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn gemma3_metadata_response_exposes_fix_and_preserves_metadata_fields() {
        for window in [1024, 512] {
            let metadata = GgufMetadata {
                architecture: Some("gemma3".into()),
                embedding_length: Some(1152),
                sliding_window: Some(window),
                ..Default::default()
            };
            let response = ModelMetadata {
                known_metadata_fixes: metadata.known_metadata_fixes(),
                metadata,
            };
            let json = serde_json::to_value(response).unwrap();
            assert_eq!(json["architecture"], "gemma3");
            assert_eq!(json["sliding_window"], window);
            if window == 1024 {
                assert_eq!(json["known_metadata_fixes"]["override_kv"], "gemma3.attention.sliding_window=int:512");
            } else {
                assert!(json["known_metadata_fixes"]["override_kv"].is_null());
            }
        }
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "harness-definition-paths-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn gguf(&self, name: &str) -> PathBuf {
            let path = self.0.join(name);
            let mut bytes = b"GGUF".to_vec();
            bytes.extend(3u32.to_le_bytes());
            bytes.extend(0u64.to_le_bytes());
            bytes.extend(1u64.to_le_bytes());
            let key = b"general.architecture";
            bytes.extend((key.len() as u64).to_le_bytes());
            bytes.extend(key);
            bytes.extend(8u32.to_le_bytes()); // GGUF string
            bytes.extend(5u64.to_le_bytes());
            bytes.extend(b"llama");
            fs::write(&path, bytes).unwrap();
            path
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn definition_paths_only_existing_requested_gguf_with_metadata() {
        let dir = Fixture::new();
        let path = dir.gguf("model-Q4_K_M.gguf");
        dir.gguf("unrelated.gguf");
        let text = dir.0.join("other.txt");
        fs::write(&text, b"text").unwrap();
        let models = inspect_model_paths(vec![
            path.clone(),
            path.clone(),
            dir.0.join("missing.gguf"),
            text,
            dir.0.clone(),
        ])
        .await
        .unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].path, path);
        assert_eq!(models[0].architecture.as_deref(), Some("llama"));
        assert!(models[0].gguf.is_some());
        assert_eq!(models[0].size_bytes, fs::metadata(&path).unwrap().len());
        let scanned = ModelManager.scan_directories(std::slice::from_ref(&dir.0));
        let same = scanned.iter().find(|m| m.path == path).unwrap();
        assert_eq!(same.size_bytes, models[0].size_bytes);
        assert_eq!(same.architecture, models[0].architecture);
    }

    #[tokio::test]
    async fn definition_paths_split_uses_first_shard_and_total_size() {
        let dir = Fixture::new();
        let first = dir.gguf("model-Q4_K_M-00001-of-00002.gguf");
        let second = dir.gguf("model-Q4_K_M-00002-of-00002.gguf");
        dir.gguf("unrelated-00001-of-00002.gguf");
        let models = inspect_model_paths(vec![second.clone(), first.clone()])
            .await
            .unwrap();
        assert_eq!(models.len(), 1);
        let model = &models[0];
        assert_eq!(model.path, first);
        assert_eq!(model.id, first.to_string_lossy());
        assert_eq!(model.shard_paths, vec![first.clone(), second.clone()]);
        assert_eq!(
            model.size_bytes,
            fs::metadata(first).unwrap().len() + fs::metadata(second).unwrap().len()
        );
        assert!(model.is_split && model.is_ready);
        assert_eq!(model.present_shards, Some(2));
        let scanned = ModelManager.scan_directories(std::slice::from_ref(&dir.0));
        let same = scanned.iter().find(|m| m.path == model.path).unwrap();
        assert_eq!(same.shard_paths, model.shard_paths);
        assert_eq!(same.size_bytes, model.size_bytes);
    }

    #[tokio::test]
    async fn definition_paths_incomplete_split_is_not_ready() {
        let dir = Fixture::new();
        let first = dir.gguf("model-00001-of-00003.gguf");
        dir.gguf("model-00003-of-00003.gguf");
        let models = inspect_model_paths(vec![first.clone()]).await.unwrap();
        assert_eq!(models[0].path, first);
        assert!(!models[0].is_ready);
        assert_eq!(models[0].present_shards, Some(2));
        assert_eq!(models[0].total_shards, Some(3));
    }
}
