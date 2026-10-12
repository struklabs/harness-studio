//! Routes and the OpenAPI document. Handlers are grouped by area; each one is a thin adapter
//! over a `harness-core` service.

mod analytics;
mod benchmarks;
mod catalog;
mod conversations;
mod convert;
mod events;
mod gateway;
mod lemonade;
mod models;
mod onnx_runtime;
mod otel;
mod runtimes;
mod servers;
mod system;
mod tokens;
mod tools;

use axum::http::{header, HeaderValue, Method};
use axum::middleware::from_fn_with_state;
use axum::Router;
use harness_core::control_api::BASE_PATH;
use tower_http::cors::{AllowOrigin, CorsLayer};
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Harness Control API",
        description = "Local management API of Harness Studio (llama.cpp control center). Every \
endpoint except /health needs `Authorization: Bearer <token>`: the session token \
(`<data dir>/api-token`, full access) or a named token with scopes (read, inference, \
models:write, downloads:write, servers:write, servers:control, conversations, admin); a token's gateway policy \
(allowed models and endpoints, rate limits) is part of it. Errors: `{\"error\": {\"code\", \"message_key\", \"params\"}}`.",
        license(name = "MIT")
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "system", description = "Health, status, hardware, runtime"),
        (name = "runtimes", description = "Runtime registry: extra llama-server builds (forks, local folders) chosen per server"),
        (name = "models", description = "Library, downloads"),
        (name = "definitions", description = "Model definitions and inference presets"),
        (name = "servers", description = "Server instances and profiles, load/unload, metrics, logs"),
        (name = "benchmarks", description = "llama-bench runs"),
        (name = "events", description = "Server-sent event streams"),
        (name = "tokens", description = "Named API tokens (admin)"),
        (name = "conversations", description = "Stored chats: list, read, export, documents, context compaction (scope `conversations`)"),
        (name = "analytics", description = "Gateway request log, statistics and Prometheus metrics"),
        (name = "tools", description = "Tool engine: list and run tools (file read/write/search, shell), the tool policy and the tool log (running needs scope `admin`)")
    )
)]
struct ApiDoc;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some("Session token or named token"))
                    .build(),
            ),
        );
        openapi.security = Some(vec![utoipa::openapi::security::SecurityRequirement::new(
            "bearer",
            Vec::<String>::new(),
        )]);
    }
}

fn api_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(system::health))
        .routes(routes!(system::status))
        .routes(routes!(system::hardware))
        .routes(routes!(system::hardware_devices))
        .routes(routes!(system::runtime))
        .routes(routes!(system::runtime_flags))
        .routes(routes!(system::managed_runtime))
        .routes(routes!(system::install_managed_runtime))
        .routes(routes!(system::rollback_managed_runtime))
        .routes(routes!(runtimes::list_runtimes, runtimes::add_runtime))
        .routes(routes!(runtimes::preview_release))
        .routes(routes!(runtimes::runtime_advice))
        .routes(routes!(runtimes::install_progress))
        .routes(routes!(runtimes::get_runtime, runtimes::remove_runtime))
        .routes(routes!(runtimes::verify_runtime))
        .routes(routes!(onnx_runtime::list_onnx_runtime))
        .routes(routes!(onnx_runtime::onnx_runtime_progress))
        .routes(routes!(
            onnx_runtime::install_onnx_runtime,
            onnx_runtime::remove_onnx_runtime
        ))
        .routes(routes!(onnx_runtime::verify_onnx_runtime))
        .routes(routes!(lemonade::lemonade_status))
        .routes(routes!(lemonade::lemonade_release))
        .routes(routes!(lemonade::lemonade_install))
        .routes(routes!(lemonade::lemonade_remove))
        .routes(routes!(lemonade::lemonade_settings))
        .routes(routes!(lemonade::lemonade_start))
        .routes(routes!(lemonade::lemonade_stop))
        .routes(routes!(lemonade::lemonade_logs))
        .routes(routes!(lemonade::lemonade_models))
        .routes(routes!(lemonade::lemonade_progress))
        .routes(routes!(lemonade::lemonade_pull))
        .routes(routes!(lemonade::lemonade_delete))
        .routes(routes!(lemonade::lemonade_install_backend))
        .routes(routes!(system::openapi_json))
        .routes(routes!(system::shutdown))
        .routes(routes!(gateway::gateway))
        .routes(routes!(models::list_models))
        .routes(routes!(models::get_model, models::delete_model))
        .routes(routes!(models::model_fit))
        .routes(routes!(models::model_automatic))
        .routes(routes!(models::pull_model))
        .routes(routes!(convert::plan))
        .routes(routes!(convert::start, convert::list))
        .routes(routes!(
            convert::env_status,
            convert::env_install,
            convert::env_remove
        ))
        .routes(routes!(convert::get_one, convert::remove))
        .routes(routes!(convert::cancel))
        .routes(routes!(models::list_downloads))
        .routes(routes!(models::get_download, models::remove_download))
        .routes(routes!(models::pause_download))
        .routes(routes!(models::resume_download))
        .routes(routes!(
            catalog::list_definitions,
            catalog::create_definition
        ))
        .routes(routes!(
            catalog::get_definition,
            catalog::update_definition,
            catalog::delete_definition
        ))
        .routes(routes!(catalog::duplicate_definition))
        .routes(routes!(catalog::modelfile_preview))
        .routes(routes!(catalog::modelfile_import))
        .routes(routes!(catalog::list_presets, catalog::create_preset))
        .routes(routes!(
            catalog::get_preset,
            catalog::update_preset,
            catalog::delete_preset
        ))
        .routes(routes!(servers::list_servers, servers::create_server))
        .routes(routes!(
            servers::get_server,
            servers::update_server,
            servers::delete_server
        ))
        .routes(routes!(servers::start_server))
        .routes(routes!(servers::stop_server))
        .routes(routes!(servers::recovery))
        .routes(routes!(servers::retry))
        .routes(routes!(servers::acknowledge_exposure))
        .routes(routes!(servers::load))
        .routes(routes!(servers::unload))
        .routes(routes!(servers::router_models))
        .routes(routes!(servers::router_load))
        .routes(routes!(servers::router_unload))
        .routes(routes!(servers::metrics))
        .routes(routes!(servers::effective))
        .routes(routes!(servers::lora_list, servers::lora_set))
        .routes(routes!(servers::logs))
        .routes(routes!(servers::logs_stream))
        .routes(routes!(
            benchmarks::list_benchmarks,
            benchmarks::start_benchmark
        ))
        .routes(routes!(benchmarks::get_benchmark))
        .routes(routes!(benchmarks::cancel_benchmark))
        .routes(routes!(events::events))
        .routes(routes!(tokens::list_tokens, tokens::create_token))
        .routes(routes!(
            tokens::get_token,
            tokens::update_token,
            tokens::revoke_token
        ))
        .routes(routes!(otel::get_otel, otel::put_otel))
        .routes(routes!(otel::test_otel))
        .routes(routes!(conversations::list))
        .routes(routes!(conversations::get_one))
        .routes(routes!(conversations::export))
        .routes(routes!(conversations::documents))
        .routes(routes!(conversations::compact))
        .routes(routes!(conversations::cancel_compact))
        .routes(routes!(conversations::set_undone))
        .routes(routes!(analytics::summary))
        .routes(routes!(analytics::requests))
        .routes(routes!(analytics::request))
        .routes(routes!(analytics::queue_hints))
        .routes(routes!(analytics::metrics))
        .routes(routes!(tools::list_tools))
        .routes(routes!(tools::inventory))
        .routes(routes!(tools::auto_state))
        .routes(routes!(tools::auto_start))
        .routes(routes!(tools::auto_action))
        .routes(routes!(tools::mcp_status))
        .routes(routes!(tools::mcp_resources))
        .routes(routes!(tools::mcp_resource))
        .routes(routes!(tools::mcp_authorize))
        .routes(routes!(tools::mcp_logout))
        .routes(routes!(tools::get_policy, tools::put_policy))
        .routes(routes!(
            tools::get_search_provider,
            tools::put_search_provider
        ))
        .routes(routes!(tools::tool_log))
        .routes(routes!(tools::run_tool))
}

fn split() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest(BASE_PATH, api_routes())
        .split_for_parts()
}

/// The OpenAPI document (paths under `/api/harness/v1`).
pub fn openapi_document() -> utoipa::openapi::OpenApi {
    split().1
}

/// The HTTP application for one listener.
pub fn router(state: AppState, cors_origins: &[String]) -> Router {
    let (router, _) = split();
    let router = router
        // MCP over streamable HTTP; behind the same auth guard.
        .route(
            &format!("{}/mcp", BASE_PATH),
            axum::routing::post(crate::mcp::post)
                .get(crate::mcp::not_allowed)
                .delete(crate::mcp::not_allowed),
        )
        .route_layer(from_fn_with_state(state.clone(), crate::auth::guard))
        .fallback(|| async {
            crate::error::ApiError::new(axum::http::StatusCode::NOT_FOUND, "api.no_route")
        })
        .with_state(state);
    if cors_origins.is_empty() {
        return router;
    }
    let origins: Vec<HeaderValue> = cors_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    router.layer(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
    )
}
