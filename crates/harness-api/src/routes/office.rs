//! Office scene collections. Reads need read; mutations and run details need admin.
use crate::{
    error::{blocking, ApiError, ApiResult, Body, ErrorBody, P},
    AppState,
};
use axum::{extract::State, http::StatusCode, Json};
use harness_core::office::{self, OfficeCharacter, OfficeScene};

#[utoipa::path(get, operation_id="list_offices", path="/offices", tag="offices", responses((status=200,body=Vec<OfficeScene>)))]
pub async fn list_offices(State(s): State<AppState>) -> ApiResult<Json<Vec<OfficeScene>>> {
    blocking(move || office::list(&s.ctx.data_dir, false))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}
#[utoipa::path(get, operation_id="get_office", path="/offices/{id}", tag="offices", params(("id"=String,Path)), responses((status=200,body=OfficeScene),(status=404,body=ErrorBody)))]
pub async fn get_offices(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<OfficeScene>> {
    let items = blocking(move || office::list(&s.ctx.data_dir, false))
        .await?
        .map_err(ApiError::core)?;
    items
        .into_iter()
        .find(|v| v.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("office", &id))
}
#[utoipa::path(put, operation_id="save_office", path="/offices/{id}", tag="offices", params(("id"=String,Path)), request_body=OfficeScene, responses((status=200,body=OfficeScene),(status=422,body=ErrorBody)))]
pub async fn save_offices(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(scene): Body<OfficeScene>,
) -> ApiResult<Json<OfficeScene>> {
    if scene.id != id {
        return Err(ApiError::core("office.invalid_scene"));
    }
    blocking(move || office::save(&s.ctx.data_dir, false, scene))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}
#[utoipa::path(delete, operation_id="delete_office", path="/offices/{id}", tag="offices", params(("id"=String,Path)), responses((status=204),(status=404,body=ErrorBody)))]
pub async fn delete_offices(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    let key = id.clone();
    if blocking(move || office::remove(&s.ctx.data_dir, false, &key))
        .await?
        .map_err(ApiError::core)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("office", &id))
    }
}

#[utoipa::path(get, operation_id="list_office_presets", path="/office-presets", tag="offices", responses((status=200,body=Vec<OfficeScene>)))]
pub async fn list_presets(State(s): State<AppState>) -> ApiResult<Json<Vec<OfficeScene>>> {
    blocking(move || office::list(&s.ctx.data_dir, true))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}
#[utoipa::path(get, operation_id="get_office_preset", path="/office-presets/{id}", tag="offices", params(("id"=String,Path)), responses((status=200,body=OfficeScene),(status=404,body=ErrorBody)))]
pub async fn get_presets(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<OfficeScene>> {
    let items = blocking(move || office::list(&s.ctx.data_dir, true))
        .await?
        .map_err(ApiError::core)?;
    items
        .into_iter()
        .find(|v| v.id == id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("office_preset", &id))
}
#[utoipa::path(put, operation_id="save_office_preset", path="/office-presets/{id}", tag="offices", params(("id"=String,Path)), request_body=OfficeScene, responses((status=200,body=OfficeScene),(status=422,body=ErrorBody)))]
pub async fn save_presets(
    State(s): State<AppState>,
    P(id): P<String>,
    Body(scene): Body<OfficeScene>,
) -> ApiResult<Json<OfficeScene>> {
    if scene.id != id {
        return Err(ApiError::core("office.invalid_scene"));
    }
    blocking(move || office::save(&s.ctx.data_dir, true, scene))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}
#[utoipa::path(delete, operation_id="delete_office_preset", path="/office-presets/{id}", tag="offices", params(("id"=String,Path)), responses((status=204),(status=404,body=ErrorBody)))]
pub async fn delete_presets(State(s): State<AppState>, P(id): P<String>) -> ApiResult<StatusCode> {
    let key = id.clone();
    if blocking(move || office::remove(&s.ctx.data_dir, true, &key))
        .await?
        .map_err(ApiError::core)?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("office_preset", &id))
    }
}

#[utoipa::path(get,operation_id="office_activity",path="/offices/activity",tag="offices",responses((status=200,body=Vec<OfficeCharacter>)))]
pub async fn activity(State(s): State<AppState>) -> ApiResult<Json<Vec<OfficeCharacter>>> {
    blocking(move || office::activity(&s.ctx.data_dir))
        .await?
        .map(Json)
        .map_err(ApiError::core)
}
#[utoipa::path(get,operation_id="office_run",path="/offices/runs/{id}",tag="offices",params(("id"=String,Path)),responses((status=200,body=serde_json::Value),(status=404,body=ErrorBody)))]
pub async fn run(
    State(s): State<AppState>,
    P(id): P<String>,
) -> ApiResult<Json<serde_json::Value>> {
    let key = id.clone();
    blocking(move || office::run(&s.ctx.data_dir, &key))
        .await?
        .map_err(ApiError::core)?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("run", &id))
}
