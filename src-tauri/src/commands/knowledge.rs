// Knowledge bases (local RAG, `harness_core::rag::kb`): named, reusable document sets built
// from a folder or files, indexed in the background (`kb-progress` events) and attached to
// conversations. The embedding model is the recommended small multilingual one
// (`harness_core::rag::embedder`) or any embedding definition of the library.

/// Knowledge bases with their index state. `embed_model` = the embedding alias in use now: a
/// base built with another one is reported `stale`.
#[tauri::command]
pub(crate) async fn kb_list(
    store: State<'_, ConversationStore>,
    embed_model: Option<String>,
) -> Result<Vec<harness_core::rag::kb::KnowledgeBase>, String> {
    let db = store.0.clone()?;
    blocking(move || {
        harness_core::rag::kb::list(&db, embed_model.as_deref().filter(|a| !a.is_empty()))
    })
    .await?
}

#[tauri::command]
pub(crate) async fn kb_create(
    store: State<'_, ConversationStore>,
    name: String,
    folder: Option<String>,
) -> Result<harness_core::rag::kb::KnowledgeBase, String> {
    let db = store.0.clone()?;
    blocking(move || -> Result<_, String> {
        let row = db
            .kb_create(&name, folder.as_deref().filter(|f| !f.is_empty()))
            .map_err(String::from)?;
        harness_core::rag::kb::get(&db, &row.id, None)
    })
    .await?
}

/// Sets the OCR model of a knowledge base (an alias of the library; `None` = no OCR). It reads
/// the PDF pages without a text layer the next time the base is indexed.
#[tauri::command]
pub(crate) async fn kb_set_ocr_model(
    store: State<'_, ConversationStore>,
    kb_id: String,
    ocr_model: Option<String>,
) -> Result<harness_core::rag::kb::KnowledgeBase, String> {
    let db = store.0.clone()?;
    blocking(move || -> Result<_, String> {
        db.kb_set_ocr_model(&kb_id, ocr_model.as_deref())
            .map_err(String::from)?;
        harness_core::rag::kb::get(&db, &kb_id, None)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn kb_rename(
    store: State<'_, ConversationStore>,
    kb_id: String,
    name: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.kb_rename(&kb_id, &name).map(|_| ())).await
}

#[tauri::command]
pub(crate) async fn kb_delete(
    store: State<'_, ConversationStore>,
    kb_id: String,
) -> Result<(), String> {
    harness_core::rag::kb::kb_jobs().cancel(&kb_id);
    with_db(&store, move |db| db.kb_delete(&kb_id)).await
}

/// Documents of a knowledge base.
#[tauri::command]
pub(crate) async fn kb_documents(
    store: State<'_, ConversationStore>,
    kb_id: String,
) -> Result<Vec<harness_core::state::RagDocument>, String> {
    with_db(&store, move |db| db.rag_documents(&kb_id)).await
}

#[derive(serde::Serialize)]
pub(crate) struct KbFolderPreview {
    files: usize,
    skipped: usize,
    truncated: bool,
}

/// How many files of a folder would be indexed.
#[tauri::command]
pub(crate) async fn kb_scan_folder(folder: String) -> Result<KbFolderPreview, String> {
    blocking(move || {
        let scan = harness_core::rag::kb::scan_folder(std::path::Path::new(&folder))?;
        Ok::<_, String>(KbFolderPreview {
            files: scan.files.len(),
            skipped: scan.skipped,
            truncated: scan.truncated,
        })
    })
    .await?
}

/// Starts (in the background) a run over a knowledge base: the files of `folder` (default: the
/// base's own folder) and `paths` are added, unfinished or outdated documents are resumed or
/// re-embedded; `rebuild` reads and embeds every document again. Progress arrives as
/// `kb-progress` events; the run stops with `kb_cancel`.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn kb_index(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    gw: State<'_, GatewayState>,
    kb_id: String,
    folder: Option<String>,
    paths: Option<Vec<String>>,
    embed_model: Option<String>,
    rebuild: bool,
) -> Result<(), String> {
    let embed = match embed_model.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => Some(rag_embed_model(&gw, alias).await?),
        None => None,
    };
    let db = store.0.clone()?;
    if harness_core::rag::kb::kb_jobs().is_running(&kb_id) {
        return Err(harness_core::error_key::err("rag.kb_busy").into());
    }
    let (files, root, ocr_alias) = {
        let db = db.clone();
        let kb_id = kb_id.clone();
        blocking(move || -> Result<_, String> {
            let kb = db.kb_get(&kb_id).map_err(String::from)?;
            let folder = folder
                .filter(|f| !f.is_empty())
                .or(kb.folder.clone())
                .map(PathBuf::from);
            let mut files: Vec<PathBuf> = Vec::new();
            let mut root = None;
            if let Some(f) = folder.filter(|f| f.is_dir()) {
                files = harness_core::rag::kb::scan_folder(&f)?.files;
                root = Some(f);
            }
            files.extend(paths.unwrap_or_default().into_iter().map(PathBuf::from));
            Ok((files, root, kb.ocr_model.clone()))
        })
        .await??
    };
    // The base's OCR model reads the PDF pages without a text layer (`rag::ocr`).
    let ocr = match ocr_alias.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => Some(harness_core::rag::ocr::OcrModel::gateway(
            rag_endpoint(&gw)
                .await
                .ok_or_else(|| String::from(harness_core::error_key::err("rag.gateway_required")))?,
            alias.to_string(),
        )),
        None => None,
    };
    let flag = harness_core::rag::kb::kb_jobs().register(&kb_id);
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        let _ = harness_core::rag::kb::index_kb(
            db,
            harness_core::rag::kb::KbIndexRequest {
                kb_id,
                files,
                root,
                embed,
                ocr,
                rebuild,
            },
            flag,
            move |p| {
                let _ = emitter.emit("kb-progress", p);
            },
        )
        .await;
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn kb_cancel(kb_id: String) -> bool {
    harness_core::rag::kb::kb_jobs().cancel(&kb_id)
}

/// Attaches a knowledge base to a conversation: its top excerpts join every question.
#[tauri::command]
pub(crate) async fn kb_attach(
    store: State<'_, ConversationStore>,
    conversation_id: String,
    kb_id: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.kb_attach(&conversation_id, &kb_id)).await
}

#[tauri::command]
pub(crate) async fn kb_detach(
    store: State<'_, ConversationStore>,
    conversation_id: String,
    kb_id: String,
) -> Result<(), String> {
    with_db(&store, move |db| db.kb_detach(&conversation_id, &kb_id)).await
}

/// Knowledge bases attached to a conversation.
#[tauri::command]
pub(crate) async fn kb_attached(
    store: State<'_, ConversationStore>,
    conversation_id: String,
    embed_model: Option<String>,
) -> Result<Vec<harness_core::rag::kb::KnowledgeBase>, String> {
    let db = store.0.clone()?;
    blocking(move || -> Result<_, String> {
        let current = embed_model.as_deref().filter(|a| !a.is_empty());
        let mut out = Vec::new();
        for row in db.kb_attached(&conversation_id).map_err(String::from)? {
            out.push(harness_core::rag::kb::get(&db, &row.id, current)?);
        }
        Ok(out)
    })
    .await?
}

/// The recommended embedding model: installed, downloading or missing.
#[tauri::command]
pub(crate) async fn embedder_status(
    download_mgr: State<'_, Arc<DownloadManager>>,
) -> Result<harness_core::rag::embedder::EmbedderStatus, String> {
    let mgr = download_mgr.inner().clone();
    blocking(move || harness_core::rag::embedder::status(&mgr)).await
}

/// Downloads the recommended embedding model (pinned revision, SHA-256 checked, about 126 MB).
#[tauri::command]
pub(crate) async fn embedder_install(
    download_mgr: State<'_, Arc<DownloadManager>>,
) -> Result<harness_core::rag::embedder::EmbedderStatus, String> {
    let mgr = download_mgr.inner().clone();
    blocking(move || {
        harness_core::rag::embedder::start_download(&mgr)?;
        Ok::<_, String>(harness_core::rag::embedder::status(&mgr))
    })
    .await?
}

/// Makes the model definition of an installed embedder (pinned to the CPU) when the download
/// finished while the app was closed; returns its alias.
#[tauri::command]
pub(crate) async fn embedder_sync() -> Result<Option<String>, String> {
    blocking(harness_core::rag::embedder::sync_definition).await?
}

/// Deletes the recommended embedding model's file.
#[tauri::command]
pub(crate) async fn embedder_remove() -> Result<(), String> {
    blocking(harness_core::rag::embedder::remove).await?
}
