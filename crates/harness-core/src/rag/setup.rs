//! What a knowledge base run or question needs from the running app, resolved without the GUI:
//! the gateway endpoint, the embedding model by alias, the OCR and speech-recognition models of
//! a base. The Control API and the CLI use this; the desktop app resolves the same things from
//! its own state.

use super::media::AsrModel;
use super::ocr::OcrModel;
use super::{kb, EmbedModel, Endpoint};
use crate::error_key::err;
use crate::state::{KnowledgeBaseRow, StateDb};
use std::path::Path;
use std::sync::Arc;

/// The managed gateway of this process with Harness's own client token; `None` when it is off.
pub fn gateway_endpoint(data_dir: &Path) -> Option<Endpoint> {
    let status = crate::gateway::status();
    let ep = status.endpoint.filter(|_| status.running)?;
    let token = std::fs::read_to_string(crate::gateway::client_token_path(data_dir))
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());
    Some(Endpoint {
        base_url: ep.base_url,
        token,
    })
}

/// The embedding definition `alias` (a library definition or a BYOK cloud embedding model) as an
/// [`EmbedModel`] on `endpoint`; `rag.model_not_found` for an unknown alias.
pub async fn embed_model(endpoint: Endpoint, alias: &str) -> Result<EmbedModel, String> {
    let wanted = alias.to_string();
    let cloud = crate::cloud::embedding_models()
        .iter()
        .any(|m| m.id == wanted)
        .then(|| {
            crate::cloud::definitions()
                .into_iter()
                .find(|d| d.alias == wanted)
        })
        .flatten();
    let def = match cloud {
        Some(def) => Some(def),
        None => {
            let w = wanted.clone();
            tokio::task::spawn_blocking(move || {
                crate::domain::store::Domain::global()
                    .definitions()
                    .map(|defs| defs.into_iter().find(|d| d.alias.eq_ignore_ascii_case(&w)))
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?
        }
    }
    .ok_or_else(|| String::from(err("rag.model_not_found").with("alias", alias)))?;
    tokio::task::spawn_blocking(move || EmbedModel::for_definition(endpoint, &def))
        .await
        .map_err(|e| e.to_string())
}

/// The OCR and speech models a base is set to, served by `endpoint`.
pub fn run_models(
    endpoint: &Endpoint,
    kb: &KnowledgeBaseRow,
) -> (Option<OcrModel>, Option<AsrModel>) {
    let ocr = kb
        .ocr_model
        .as_deref()
        .filter(|a| !a.is_empty())
        .map(|a| OcrModel::gateway(endpoint.clone(), a.to_string()));
    let asr = kb
        .asr_model
        .as_deref()
        .filter(|a| !a.is_empty())
        .map(|a| AsrModel {
            endpoint: endpoint.clone(),
            alias: a.to_string(),
        });
    (ocr, asr)
}

/// Starts a run in the background (`rag.kb_busy` when the base is being indexed already); the
/// run's progress is readable with [`kb::progress_of`] and the run stops with
/// `kb::kb_jobs().cancel(id)`.
pub fn spawn_run(
    db: Arc<StateDb>,
    req: kb::KbIndexRequest,
    on_progress: impl Fn(kb::KbProgress) + Send + Sync + 'static,
) -> Result<(), String> {
    if kb::kb_jobs().is_running(&req.kb_id) {
        return Err(err("rag.kb_busy").into());
    }
    let flag = kb::kb_jobs().register(&req.kb_id);
    tokio::spawn(async move {
        let _ = kb::index_kb(db, req, flag, on_progress).await;
    });
    Ok(())
}
