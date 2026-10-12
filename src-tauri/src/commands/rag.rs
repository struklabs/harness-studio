// Playground documents (local RAG, `harness_core::rag`): attach, index, retrieve.
// Embedding and rerank requests go to the gateway (JIT load by alias); without the gateway
// documents still work as full text or keyword search.

#[derive(serde::Serialize)]
pub(crate) struct RagModels {
    embedding: Vec<harness_core::rag::models::RetrievalModel>,
    rerank: Vec<harness_core::rag::models::RetrievalModel>,
    /// The gateway runs (embedding and rerank need it).
    gateway: bool,
}

/// Embedding and rerank definitions of the library (by GGUF header).
#[tauri::command]
pub(crate) async fn rag_models(gw: State<'_, GatewayState>) -> Result<RagModels, String> {
    use harness_core::rag::models::{retrieval_models, RetrievalRole};
    let all = blocking(|| {
        harness_core::domain::store::Domain::global()
            .definitions()
            .map(|defs| retrieval_models(&defs))
    })
    .await??;
    let (embedding, rerank) = all
        .into_iter()
        .partition(|m| m.role == RetrievalRole::Embedding);
    Ok(RagModels {
        embedding,
        rerank,
        gateway: gw.server.status().await.running,
    })
}

async fn rag_endpoint(gw: &GatewayState) -> Option<harness_core::rag::Endpoint> {
    let status = gw.server.status().await;
    let ep = status.endpoint.filter(|_| status.running)?;
    Some(harness_core::rag::Endpoint {
        base_url: ep.base_url,
        token: gw.server.client_token().await,
    })
}

/// The embedding definition `alias` with the gateway endpoint; `rag.gateway_required` when
/// the gateway is off, `rag.model_not_found` for an unknown alias.
async fn rag_embed_model(
    gw: &GatewayState,
    alias: &str,
) -> Result<harness_core::rag::EmbedModel, String> {
    let endpoint = rag_endpoint(gw)
        .await
        .ok_or_else(|| String::from(harness_core::error_key::err("rag.gateway_required")))?;
    let wanted = alias.to_string();
    let def = blocking(move || {
        harness_core::domain::store::Domain::global()
            .definitions()
            .map(|defs| {
                defs.into_iter()
                    .find(|d| d.alias.eq_ignore_ascii_case(&wanted))
            })
    })
    .await??
    .ok_or_else(|| {
        String::from(harness_core::error_key::err("rag.model_not_found").with("alias", alias))
    })?;
    blocking(move || harness_core::rag::EmbedModel::for_definition(endpoint, &def)).await
}

/// Starts (in the background) the indexing job of a registered document.
fn spawn_rag_job(
    app: tauri::AppHandle,
    db: Arc<StateDb>,
    job: harness_core::rag::IndexJob,
    flag: Arc<std::sync::atomic::AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let emitter = app.clone();
        let _ = harness_core::rag::run_job(db, job, flag, move |p| {
            let _ = emitter.emit("rag-progress", p);
        })
        .await;
    });
}

/// Documents of a conversation. Documents left "indexing" by a closed app become cancelled.
#[tauri::command]
pub(crate) async fn rag_list_documents(
    store: State<'_, ConversationStore>,
    conversation_id: String,
) -> Result<Vec<harness_core::state::RagDocument>, String> {
    with_db(&store, move |db| {
        db.rag_reconcile(&conversation_id, |id| {
            harness_core::rag::jobs().is_running(id)
        })?;
        db.rag_documents(&conversation_id)
    })
    .await
}

/// File picker for documents (several at once).
#[tauri::command]
pub(crate) async fn rag_pick_files(app: tauri::AppHandle, label: String) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    let exts = harness_core::rag::extract::supported_extensions();
    blocking(move || {
        app.dialog()
            .file()
            .add_filter(label, &exts)
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// File picker for pictures (several at once). The webview's own `<input type="file">` does not
/// open a chooser in the packaged Windows app, so the Playground uses the native dialog.
#[tauri::command]
pub(crate) async fn pick_image_files(app: tauri::AppHandle, label: String) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    blocking(move || {
        app.dialog()
            .file()
            .add_filter(label, &["png", "jpg", "jpeg", "gif", "bmp", "webp", "avif"])
            .blocking_pick_files()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// A picture dropped on the Playground (the window hands over paths, not files): its bytes as
/// base64, for the page to scale and send to a vision model. Only picture files, 25 MB at most.
#[tauri::command]
pub(crate) async fn read_image_file(path: String) -> Result<String, String> {
    use base64::Engine;
    const MAX_BYTES: u64 = 25 * 1024 * 1024;
    blocking(move || {
        let p = PathBuf::from(&path);
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !["png", "jpg", "jpeg", "gif", "bmp", "webp", "avif"].contains(&ext.as_str()) {
            return Err(format!("not a picture file: {path}"));
        }
        let size = std::fs::metadata(&p).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err(format!("picture larger than 25 MB: {path}"));
        }
        let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    })
    .await?
}

/// Attaches files to a conversation and indexes them one after the other in the background
/// (`rag-progress` events). `embed_model` = embedding definition alias (needs the gateway);
/// without it documents are searchable by keywords and usable as full text.
#[tauri::command]
pub(crate) async fn rag_add_files(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    gw: State<'_, GatewayState>,
    conversation_id: String,
    paths: Vec<String>,
    embed_model: Option<String>,
) -> Result<Vec<harness_core::state::RagDocument>, String> {
    let embed = match embed_model.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => Some(rag_embed_model(&gw, alias).await?),
        None => None,
    };
    let db = store.0.clone()?;
    let mut out = Vec::new();
    for path in paths {
        let p = PathBuf::from(&path);
        let name = harness_core::rag::extract::file_name(&p);
        let conv = conversation_id.clone();
        let doc = {
            let db = db.clone();
            let path = path.clone();
            blocking(move || db.rag_create_document(&conv, &name, Some(&path)))
                .await?
                .map_err(String::from)?
        };
        let flag = harness_core::rag::jobs().register(&doc.id);
        spawn_rag_job(
            app.clone(),
            db.clone(),
            harness_core::rag::IndexJob {
                document_id: doc.id.clone(),
                source: Some(p),
                embed: embed.clone(),
            },
            flag,
        );
        out.push(doc);
    }
    Ok(out)
}

/// Re-embeds documents with `embed_model` (after a model change) or resumes cancelled /
/// failed ones. `document_ids` = none given: every document of the conversation that is
/// not indexing now and is not already embedded with that model.
#[tauri::command]
pub(crate) async fn rag_reindex(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    gw: State<'_, GatewayState>,
    conversation_id: String,
    document_ids: Option<Vec<String>>,
    embed_model: Option<String>,
) -> Result<Vec<String>, String> {
    let embed = match embed_model.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => Some(rag_embed_model(&gw, alias).await?),
        None => None,
    };
    let db = store.0.clone()?;
    let docs = {
        let db = db.clone();
        let conv = conversation_id.clone();
        blocking(move || db.rag_documents(&conv)).await?.map_err(String::from)?
    };
    let mut started = Vec::new();
    for d in docs {
        if harness_core::rag::jobs().is_running(&d.id) {
            continue;
        }
        let wanted = match &document_ids {
            Some(ids) => ids.contains(&d.id),
            None => {
                d.status != "ready"
                    || embed.as_ref().is_some_and(|e| {
                        d.embed_model.as_deref() != Some(e.alias.as_str()) || d.embedded < d.chunks
                    })
            }
        };
        if !wanted {
            continue;
        }
        let flag = harness_core::rag::jobs().register(&d.id);
        spawn_rag_job(
            app.clone(),
            db.clone(),
            harness_core::rag::IndexJob {
                document_id: d.id.clone(),
                source: None,
                embed: embed.clone(),
            },
            flag,
        );
        started.push(d.id);
    }
    Ok(started)
}

#[tauri::command]
pub(crate) fn rag_cancel(document_id: String) -> bool {
    harness_core::rag::jobs().cancel(&document_id)
}

#[tauri::command]
pub(crate) async fn rag_delete_document(
    store: State<'_, ConversationStore>,
    document_id: String,
) -> Result<(), String> {
    harness_core::rag::jobs().cancel(&document_id);
    with_db(&store, move |db| db.rag_delete_document(&document_id)).await
}

/// `auto` (full text when it fits, else search), `full` or `retrieval`.
#[tauri::command]
pub(crate) async fn rag_set_document_mode(
    store: State<'_, ConversationStore>,
    document_id: String,
    mode: String,
) -> Result<harness_core::state::RagDocument, String> {
    with_db(&store, move |db| db.rag_set_mode(&document_id, &mode)).await
}

#[derive(serde::Deserialize)]
pub(crate) struct RagPrepareInput {
    conversation_id: String,
    query: String,
    top_k: usize,
    context_tokens: Option<usize>,
    embed_model: Option<String>,
    rerank_model: Option<String>,
}

/// The document context of one chat turn (full-text system block and/or numbered
/// excerpts) with its sources. Embedding/rerank run only while the gateway is up; otherwise
/// keyword search is used and a notice says why.
#[tauri::command]
pub(crate) async fn rag_prepare(
    store: State<'_, ConversationStore>,
    gw: State<'_, GatewayState>,
    input: RagPrepareInput,
) -> Result<harness_core::rag::Prepared, String> {
    let db = store.0.clone()?;
    let mut notices = Vec::new();
    let embed = match input.embed_model.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => match rag_embed_model(&gw, alias).await {
            Ok(m) => Some(m),
            Err(e) => {
                notices.push(e);
                None
            }
        },
        None => None,
    };
    let rerank = match input.rerank_model.as_deref().filter(|a| !a.is_empty()) {
        Some(alias) => rag_endpoint(&gw).await.map(|ep| (ep, alias.to_string())),
        None => None,
    };
    let opts = harness_core::rag::RetrieveOptions {
        query: input.query,
        top_k: input.top_k,
        context_tokens: input.context_tokens,
        embed,
        rerank,
    };
    let mut prepared = harness_core::rag::prepare(&db, &input.conversation_id, &opts).await?;
    if prepared.retrieval.is_some() {
        notices.append(&mut prepared.notices);
        prepared.notices = notices;
    }
    Ok(prepared)
}
