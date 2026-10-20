//! Bundled modules: reads require read, installation/removal/cancellation require admin.
use crate::error::{blocking, ApiResult, ErrorBody, P};
use axum::{http::StatusCode, Json};
use harness_core::modules::{self, ModuleView};

#[utoipa::path(get, path = "/modules", tag = "modules",
    responses((status = 200, body = Vec<ModuleView>), (status = 401, body = ErrorBody)))]
pub async fn list_modules() -> ApiResult<Json<Vec<ModuleView>>> {
    Ok(Json(blocking(modules::list).await??))
}
#[utoipa::path(post, path = "/modules/{id}", tag = "modules", params(("id" = String, Path)),
    responses((status = 204), (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn install_module(P(id): P<String>) -> ApiResult<StatusCode> {
    modules::install(&id).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(post, path = "/modules/{id}/cancel", tag = "modules", params(("id" = String, Path)),
    responses((status = 204), (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn cancel_module(P(id): P<String>) -> ApiResult<StatusCode> {
    modules::cancel(&id)?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(delete, path = "/modules/{id}", tag = "modules", params(("id" = String, Path)),
    responses((status = 204), (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn remove_module(P(id): P<String>) -> ApiResult<StatusCode> {
    blocking(move || modules::remove(&id)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize, utoipa::ToSchema)]
pub struct ModuleAction {
    /// install, update, repair, enable, or disable
    action: String,
}
#[utoipa::path(post, path = "/modules/{id}/action", tag = "modules", params(("id" = String, Path)), request_body = ModuleAction,
    responses((status = 204), (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn module_action(
    P(id): P<String>,
    Json(body): Json<ModuleAction>,
) -> ApiResult<StatusCode> {
    modules::action(&id, &body.action).await?;
    Ok(StatusCode::NO_CONTENT)
}
#[utoipa::path(post, path = "/modules/{id}/remove-data", tag = "modules", params(("id" = String, Path)), request_body = modules::RemovalOptions,
    responses((status = 204), (status = 422, body = ErrorBody), (status = 403, body = ErrorBody)))]
pub async fn remove_module_data(
    P(id): P<String>,
    Json(options): Json<modules::RemovalOptions>,
) -> ApiResult<StatusCode> {
    blocking(move || modules::lifecycle_remove(&id, &options)).await??;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    #[test]
    fn openapi_documents_module_lifecycle_routes() {
        let doc = serde_json::to_value(crate::openapi_document()).unwrap();
        for (route, schema) in [
            ("action", "ModuleAction"),
            ("remove-data", "RemovalOptions"),
        ] {
            let path = format!(
                "{}/modules/{{id}}/{route}",
                harness_core::control_api::BASE_PATH
            );
            let operation = &doc["paths"][&path]["post"];
            assert!(operation["responses"]["204"].is_object(), "{path}");
            assert_eq!(
                operation["requestBody"]["content"]["application/json"]["schema"]["$ref"],
                format!("#/components/schemas/{schema}"),
                "{path}"
            );
        }
    }
}
