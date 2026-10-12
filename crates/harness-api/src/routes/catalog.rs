//! Model definitions and inference presets (`Domain`, same calls as the GUI).

use axum::http::StatusCode;
use axum::Json;
use harness_core::domain::store::Domain;
use harness_core::domain::{InferencePreset, ModelDefinition};
use harness_core::modelfile::{ModelfileImport, ModelfileRequest};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::error::{blocking, with_id, ApiError, ApiResult, Body, ErrorBody, OptBody, P};

fn definition_exists(domain: &Domain, id: &str) -> Result<bool, String> {
    Ok(domain.definitions()?.iter().any(|d| d.id == id))
}

#[utoipa::path(get, path = "/definitions", tag = "definitions",
    responses((status = 200, body = Vec<ModelDefinition>)))]
pub async fn list_definitions() -> ApiResult<Json<Vec<ModelDefinition>>> {
    Ok(Json(blocking(|| Domain::global().definitions()).await??))
}

/// Creates a definition (`id` is assigned; any given id is ignored).
#[utoipa::path(post, path = "/definitions", tag = "definitions", request_body = ModelDefinition,
    responses((status = 201, body = ModelDefinition), (status = 422, body = ErrorBody)))]
pub async fn create_definition(
    Body(body): Body<serde_json::Value>,
) -> ApiResult<(StatusCode, Json<ModelDefinition>)> {
    let def: ModelDefinition = with_id(body, "")?;
    let saved = blocking(move || Domain::global().save_definition(def)).await??;
    Ok((StatusCode::CREATED, Json(saved)))
}

#[utoipa::path(get, path = "/definitions/{id}", tag = "definitions",
    params(("id" = String, Path)),
    responses((status = 200, body = ModelDefinition), (status = 404, body = ErrorBody)))]
pub async fn get_definition(P(id): P<String>) -> ApiResult<Json<ModelDefinition>> {
    let found = blocking({
        let id = id.clone();
        move || {
            Domain::global()
                .definitions()
                .map(|l| l.into_iter().find(|d| d.id == id))
        }
    })
    .await??;
    found
        .map(Json)
        .ok_or_else(|| ApiError::not_found("definition", &id))
}

/// Replaces a definition (the body's `id` is ignored).
#[utoipa::path(put, path = "/definitions/{id}", tag = "definitions",
    params(("id" = String, Path)), request_body = ModelDefinition,
    responses((status = 200, body = ModelDefinition), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn update_definition(
    P(id): P<String>,
    Body(body): Body<serde_json::Value>,
) -> ApiResult<Json<ModelDefinition>> {
    let def: ModelDefinition = with_id(body, &id)?;
    let saved = blocking({
        let id = id.clone();
        move || {
            let domain = Domain::global();
            if !definition_exists(&domain, &id)? {
                return Ok(None);
            }
            domain.save_definition(def).map(Some)
        }
    })
    .await??;
    saved
        .map(Json)
        .ok_or_else(|| ApiError::not_found("definition", &id))
}

/// Refused while a profile serves it.
#[utoipa::path(delete, path = "/definitions/{id}", tag = "definitions",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn delete_definition(P(id): P<String>) -> ApiResult<StatusCode> {
    let missing = blocking({
        let id = id.clone();
        move || {
            let domain = Domain::global();
            if !definition_exists(&domain, &id)? {
                return Ok(true);
            }
            domain.delete_definition(&id).map(|_| false)
        }
    })
    .await??;
    if missing {
        return Err(ApiError::not_found("definition", &id));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, ToSchema, Default)]
pub struct DuplicateRequest {
    /// Alias of the copy (default: a free `<alias>-2`...).
    pub alias: Option<String>,
}

#[utoipa::path(post, path = "/definitions/{id}/duplicate", tag = "definitions",
    params(("id" = String, Path)), request_body = DuplicateRequest,
    responses((status = 201, body = ModelDefinition), (status = 404, body = ErrorBody)))]
pub async fn duplicate_definition(
    P(id): P<String>,
    OptBody(body): OptBody<DuplicateRequest>,
) -> ApiResult<(StatusCode, Json<ModelDefinition>)> {
    let alias = body.and_then(|b| b.alias);
    let copy = blocking({
        let id = id.clone();
        move || {
            let domain = Domain::global();
            if !definition_exists(&domain, &id)? {
                return Ok(None);
            }
            domain.duplicate_definition(&id, alias).map(Some)
        }
    })
    .await??;
    copy.map(|d| (StatusCode::CREATED, Json(d)))
        .ok_or_else(|| ApiError::not_found("definition", &id))
}

// --- Ollama Modelfile import ---

fn taken_aliases(domain: &Domain) -> Result<Vec<String>, String> {
    Ok(domain.definitions()?.into_iter().map(|d| d.alias).collect())
}

/// Reads an Ollama Modelfile and returns what importing it would create (definition, preset,
/// mapped values, warnings). Nothing is saved.
#[utoipa::path(post, path = "/modelfile/preview", tag = "definitions", request_body = ModelfileRequest,
    responses((status = 200, body = serde_json::Value), (status = 422, body = ErrorBody)))]
pub async fn modelfile_preview(
    Body(req): Body<ModelfileRequest>,
) -> ApiResult<Json<ModelfileImport>> {
    let preview = blocking(move || req.preview(taken_aliases(&Domain::global())?)).await??;
    Ok(Json(preview))
}

/// Imports a Modelfile: creates the model definition (alias from the request, else derived
/// from `FROM`) and its inference preset. Returns the saved definition and the warnings.
#[utoipa::path(post, path = "/modelfile/import", tag = "definitions", request_body = ModelfileRequest,
    responses((status = 201, body = serde_json::Value), (status = 422, body = ErrorBody)))]
pub async fn modelfile_import(
    Body(req): Body<ModelfileRequest>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let (def, warnings) = blocking(move || {
        let domain = Domain::global();
        let wanted = req.alias.clone();
        let preview = req.preview(taken_aliases(&domain)?)?;
        let alias = wanted.unwrap_or_else(|| preview.import.suggested_alias.clone());
        let def = domain.import_definition(preview.import, &alias)?;
        Ok::<_, String>((def, preview.warnings))
    })
    .await??;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "definition": def, "warnings": warnings })),
    ))
}

// --- Inference presets ---

#[utoipa::path(get, path = "/presets", tag = "definitions",
    responses((status = 200, description = "Built-in presets (read-only) first, then the user's",
        body = Vec<InferencePreset>)))]
pub async fn list_presets() -> ApiResult<Json<Vec<InferencePreset>>> {
    Ok(Json(
        blocking(|| Domain::global().inference_presets()).await??,
    ))
}

fn find_preset(id: &str) -> Result<Option<InferencePreset>, String> {
    Ok(Domain::global()
        .inference_presets()?
        .into_iter()
        .find(|p| p.id == id))
}

#[utoipa::path(post, path = "/presets", tag = "definitions", request_body = InferencePreset,
    responses((status = 201, body = InferencePreset), (status = 422, body = ErrorBody)))]
pub async fn create_preset(
    Body(body): Body<serde_json::Value>,
) -> ApiResult<(StatusCode, Json<InferencePreset>)> {
    let mut preset: InferencePreset = with_id(body, "")?;
    preset.builtin = false;
    let saved = blocking(move || Domain::global().save_inference_preset(preset)).await??;
    Ok((StatusCode::CREATED, Json(saved)))
}

#[utoipa::path(get, path = "/presets/{id}", tag = "definitions",
    params(("id" = String, Path)),
    responses((status = 200, body = InferencePreset), (status = 404, body = ErrorBody)))]
pub async fn get_preset(P(id): P<String>) -> ApiResult<Json<InferencePreset>> {
    let found = blocking({
        let id = id.clone();
        move || find_preset(&id)
    })
    .await??;
    found
        .map(Json)
        .ok_or_else(|| ApiError::not_found("preset", &id))
}

/// Replaces a user preset (built-ins are read-only).
#[utoipa::path(put, path = "/presets/{id}", tag = "definitions",
    params(("id" = String, Path)), request_body = InferencePreset,
    responses((status = 200, body = InferencePreset), (status = 404, body = ErrorBody),
        (status = 422, body = ErrorBody)))]
pub async fn update_preset(
    P(id): P<String>,
    Body(body): Body<serde_json::Value>,
) -> ApiResult<Json<InferencePreset>> {
    let preset: InferencePreset = with_id(body, &id)?;
    let saved = blocking({
        let id = id.clone();
        move || {
            if find_preset(&id)?.is_none() {
                return Ok(None);
            }
            Domain::global().save_inference_preset(preset).map(Some)
        }
    })
    .await??;
    saved
        .map(Json)
        .ok_or_else(|| ApiError::not_found("preset", &id))
}

#[utoipa::path(delete, path = "/presets/{id}", tag = "definitions",
    params(("id" = String, Path)),
    responses((status = 204), (status = 404, body = ErrorBody), (status = 422, body = ErrorBody)))]
pub async fn delete_preset(P(id): P<String>) -> ApiResult<StatusCode> {
    let missing = blocking({
        let id = id.clone();
        move || {
            if find_preset(&id)?.is_none() {
                return Ok(true);
            }
            Domain::global().delete_inference_preset(&id).map(|_| false)
        }
    })
    .await??;
    if missing {
        return Err(ApiError::not_found("preset", &id));
    }
    Ok(StatusCode::NO_CONTENT)
}
