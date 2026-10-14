// My Models (Kütüphanem): library listing with user state, favorites, reveal in folder,
// safe delete with preview, duplicates, missing-part repair and draft suggestions.
// Thin wrappers over `harness_core::library`.

use harness_core::library::{self as lib, draft, dupes, ops};

fn state_db() -> (Option<Arc<StateDb>>, Option<String>) {
    match crate::state::shared() {
        Ok(db) => (Some(db), None),
        Err(e) => (None, Some(e)),
    }
}

fn find_library_model(mgr: &ModelManager, model_id: &str) -> Result<LocalModel, String> {
    mgr.find_model(model_id)
        .ok_or_else(|| harness_core::error_key::err("models.not_found").with("id", model_id).into())
}

/// Every local model with its parameters, capabilities, pairs, state and draft targets.
#[tauri::command]
pub(crate) async fn list_library() -> Result<lib::LibraryView, String> {
    blocking(|| {
        let (db, err) = state_db();
        let mut view = lib::list(db.as_deref(), err);
        // Whether the runtime family a file needs is installed is a local fact, settled here
        // the same way as for the Hub listing.
        let installed = harness_core::model_registry::installed_families();
        for entry in &mut view.entries {
            entry.record.resolve_installed(&installed);
        }
        view
    })
    .await
}

#[tauri::command]
pub(crate) async fn set_model_favorite(model_id: String, favorite: bool) -> Result<(), String> {
    blocking(move || {
        let db = crate::state::shared()?;
        db.set_model_favorite(&lib::state_key(Path::new(&model_id)), favorite)
            .map_err(String::from)
    })
    .await?
}

/// Opens the file manager at the model file (only files found by a library scan).
#[tauri::command]
pub(crate) async fn reveal_model_in_folder(
    model_mgr: State<'_, Arc<ModelManager>>,
    model_id: String,
) -> Result<(), String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || {
        let model = find_library_model(&mgr, &model_id)?;
        ops::reveal_in_folder(&model.path)
    })
    .await?
}

/// Files, definitions, profiles and running servers a delete would touch.
#[tauri::command]
pub(crate) async fn get_model_delete_preview(
    model_mgr: State<'_, Arc<ModelManager>>,
    registry: State<'_, Arc<ServerRegistry>>,
    model_id: String,
) -> Result<ops::DeletePreview, String> {
    let mgr = model_mgr.inner().clone();
    let registry = registry.inner().clone();
    blocking(move || {
        let model = find_library_model(&mgr, &model_id)?;
        let domain = harness_core::domain::store::Domain::global();
        let running = ops::servers_using_model(&registry, &model);
        Ok(ops::delete_preview(
            &model,
            &domain.definitions()?,
            &domain.profiles()?,
            running,
        ))
    })
    .await?
}

/// Deletes a library model (all parts). Refused while any server uses one of its files, for
/// linked models (unlink them instead) and for files outside the library directories.
#[tauri::command]
pub(crate) async fn delete_library_model(
    registry: State<'_, Arc<ServerRegistry>>,
    gate: State<'_, Arc<OperationGate>>,
    model_id: String,
    delete_definitions: bool,
) -> Result<ops::DeleteReport, String> {
    crate::orchestrator::delete_library_model(&registry, &gate, model_id, delete_definitions).await
}

/// Groups of identical-looking models (same size, same first/last MiB).
#[tauri::command]
pub(crate) async fn find_duplicate_models(
    model_mgr: State<'_, Arc<ModelManager>>,
) -> Result<Vec<dupes::DuplicateGroup>, String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || dupes::find_duplicates(&mgr.scan_models())).await
}

/// Full SHA-256 of the given models ("Tam doğrula"); recorded download hashes are reused.
#[tauri::command]
pub(crate) async fn verify_duplicate_models(
    model_mgr: State<'_, Arc<ModelManager>>,
    model_ids: Vec<String>,
) -> Result<Vec<dupes::VerifiedMember>, String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || {
        let models: Vec<LocalModel> = mgr
            .scan_models()
            .into_iter()
            .filter(|m| model_ids.contains(&m.id))
            .collect();
        dupes::verify(&models, &crate::downloader::recorded_hashes())
    })
    .await?
}

/// Queues the missing parts of a split model: from its Hugging Face repo folder, the recorded
/// download address of a part, or `url` (the address of one part, for files without a record).
#[tauri::command]
pub(crate) async fn repair_model_shards(
    model_mgr: State<'_, Arc<ModelManager>>,
    download_mgr: State<'_, Arc<DownloadManager>>,
    model_id: String,
    url: Option<String>,
) -> Result<u64, String> {
    let mgr = model_mgr.inner().clone();
    let model = blocking(move || find_library_model(&mgr, &model_id)).await??;
    let settings = AppSettings::load();
    let token = harness_core::hf_auth::current_token().await;
    ops::repair(
        &download_mgr,
        &model,
        &settings.all_models_dirs(),
        token.as_deref(),
        url.as_deref(),
    )
    .await
}

/// Local LLMs that can draft for the model at `model_path` (same tokenizer, much smaller).
#[tauri::command]
pub(crate) async fn suggest_draft_models(
    model_mgr: State<'_, Arc<ModelManager>>,
    model_path: String,
) -> Result<Vec<draft::DraftSuggestion>, String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || draft::suggest_for(Path::new(&model_path), &mgr.scan_models())).await
}

/// The projector (mmproj) file that belongs to the model at `model_path`, found by the
/// library's own pairing (same folder, same basename or name); `None` when there is none.
#[tauri::command]
pub(crate) async fn suggest_projector(
    model_mgr: State<'_, Arc<ModelManager>>,
    model_path: String,
) -> Result<Option<harness_core::library::ProjectorSuggestion>, String> {
    let mgr = model_mgr.inner().clone();
    blocking(move || {
        harness_core::library::suggest_projector(Path::new(&model_path), &mgr.scan_models())
    })
    .await
}
