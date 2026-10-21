//! Routes and the OpenAPI document. Handlers are grouped by area; each one is a thin adapter
//! over a `harness-core` service.

mod analytics;
mod automations;
mod benchmarks;
mod catalog;
mod cloud;
mod conversations;
mod convert;
mod events;
mod gateway;
mod lemonade;
mod models;
mod modules;
mod onnx_runtime;
mod otel;
mod permissions;
mod runtimes;
mod servers;
mod settings;
mod system;
mod tokens;
mod tools;
mod training;

use std::sync::Arc;

use axum::http::{header, HeaderValue, Method};
use axum::middleware::from_fn_with_state;
use axum::Router;
use harness_core::control_api::BASE_PATH;
use tower_http::cors::{AllowOrigin, CorsLayer};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Harness Control API",
        description = "Local management API of Harness Studio (llama.cpp control center). Every \
protected endpoint accepts `Authorization: Bearer <token>`: the local owner token \
(`<data dir>/api-token`, full access) or a named token with scopes (read, inference, \
models:write, downloads:write, servers:write, servers:control, conversations, admin); a token's gateway policy \
(allowed models and endpoints, rate limits) is part of it. Headless browser administrators may instead use \
the Secure HttpOnly session cookie; cookie writes need same-origin Origin and X-Harness-CSRF. \
/health and browser login/session bootstrap are public. Errors: `{\"error\": {\"code\", \"message_key\", \"params\"}}`.",
        license(name = "MIT")
    ),
    modifiers(&BearerAuth),
    tags(
        (name = "system", description = "Health, status, hardware, runtime"),
        (name = "runtimes", description = "Runtime registry: extra llama-server builds (forks, local folders) chosen per server"),
        (name = "modules", description = "Bundled optional modules: read to inspect; admin to install, cancel, or remove"),
        (name = "models", description = "Library, downloads"),
        (name = "definitions", description = "Model definitions and inference presets"),
        (name = "servers", description = "Server instances and profiles, load/unload, metrics, logs"),
        (name = "benchmarks", description = "llama-bench runs"),
        (name = "events", description = "Server-sent event streams"),
        (name = "tokens", description = "Named API tokens (admin)"),
        (name = "cloud", description = "BYOK cloud providers (OpenAI, Gemini, Anthropic, OpenRouter, custom): add, test and remove; the key is write-only (admin)"),
        (name = "conversations", description = "Stored chats: list, read, export, documents, context compaction (scope `conversations`)"),
        (name = "training", description = "Training jobs: LoRA / QLoRA fine-tuning of a library model on a JSONL dataset, with loss history, checkpoints, resume and export to the library (admin to create, run and export)"),
        (name = "analytics", description = "Gateway request log, statistics and Prometheus metrics"),
        (name = "automations", description = "The user's own n8n instance: connection, workflows, executions, webhook runs (admin)"),
        (name = "permissions", description = "Trust levels and capability grants of MCP servers, agent runtimes and the training backend (admin)"),
        (name = "tools", description = "Tool engine: list and run tools (file read/write/search, shell), the tool policy and the tool log (running needs scope `admin`)")
    )
)]
struct ApiDoc;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        // The contract's major version (`/v1`), which `sdk/check-breaking.mjs` reads: a
        // breaking change to the frozen contract is only allowed together with a new major.
        openapi.info.extensions = Some(
            utoipa::openapi::extensions::ExtensionsBuilder::new()
                .add(
                    "x-harness-api-version",
                    harness_core::control_api::API_VERSION,
                )
                .build(),
        );
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
        components.add_security_scheme(
            "browser_cookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::with_description(
                "__Host-harness",
                "Serve-only administrator session. Writes also require same-origin Origin and X-Harness-CSRF from /browser/session. Secure, HttpOnly, SameSite=Strict.",
            ))),
        );
        openapi.security = Some(vec![
            utoipa::openapi::security::SecurityRequirement::new("bearer", Vec::<String>::new()),
            utoipa::openapi::security::SecurityRequirement::new(
                "browser_cookie",
                Vec::<String>::new(),
            ),
        ]);
    }
}

fn api_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(crate::operations::service_status))
        .routes(routes!(crate::operations::service_action))
        .routes(routes!(crate::operations::logs))
        .routes(routes!(crate::operations::backup))
        .routes(routes!(crate::operations::restore))
        .routes(routes!(crate::operations::update_check))
        .routes(routes!(crate::operations::update_apply))
        .routes(routes!(crate::browser::session))
        .routes(routes!(crate::browser::login))
        .routes(routes!(crate::browser::logout))
        .routes(routes!(crate::browser::revoke))
        .routes(routes!(
            crate::browser::settings,
            crate::browser::save_settings
        ))
        .routes(routes!(modules::list_modules))
        .routes(routes!(modules::install_module, modules::remove_module))
        .routes(routes!(modules::cancel_module))
        .routes(routes!(modules::module_action))
        .routes(routes!(modules::remove_module_data))
        .routes(routes!(modules::export_module))
        .routes(routes!(modules::import_module))
        .routes(routes!(system::health))
        .routes(routes!(system::capabilities))
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
        .routes(routes!(gateway::jobs))
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
        .routes(routes!(training::check_dataset))
        .routes(routes!(training::preview))
        .routes(routes!(training::presets))
        .routes(routes!(training::check_name))
        .routes(routes!(training::rename))
        .routes(routes!(training::compatibility))
        .routes(routes!(training::env_status, training::env_install))
        .routes(routes!(training::list, training::create))
        .routes(routes!(training::get_one, training::remove))
        .routes(routes!(training::metrics))
        .routes(routes!(training::log))
        .routes(routes!(training::start))
        .routes(routes!(training::cancel))
        .routes(routes!(training::resume))
        .routes(routes!(training::restart))
        .routes(routes!(training::export))
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
        .routes(routes!(servers::api_key))
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
        .routes(routes!(
            cloud::list_cloud_providers,
            cloud::add_cloud_provider
        ))
        .routes(routes!(
            cloud::update_cloud_provider,
            cloud::remove_cloud_provider
        ))
        .routes(routes!(cloud::test_cloud_provider))
        .routes(routes!(cloud::list_cloud_models))
        .routes(routes!(otel::get_otel, otel::put_otel))
        .routes(routes!(otel::test_otel))
        .routes(routes!(automations::get_n8n, automations::put_n8n))
        .routes(routes!(automations::test_n8n))
        .routes(routes!(automations::n8n_workflows))
        .routes(routes!(automations::n8n_executions))
        .routes(routes!(automations::n8n_execution_error))
        .routes(routes!(automations::trigger_n8n))
        .routes(routes!(automations::n8n_openai))
        .routes(routes!(automations::put_n8n_permission))
        .routes(routes!(automations::n8n_proposals))
        .routes(routes!(automations::n8n_proposal))
        .routes(routes!(automations::apply_n8n_proposal))
        .routes(routes!(automations::reject_n8n_proposal))
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
        .routes(routes!(settings::get, settings::update))
        .routes(routes!(permissions::list))
        .routes(routes!(permissions::grant))
        .routes(routes!(permissions::revoke_grant))
        .routes(routes!(permissions::revoke_subject))
        .routes(routes!(permissions::set_trust))
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
        .routes(routes!(tools::registry_list))
        .routes(routes!(tools::registry_install))
        .routes(routes!(tools::installed_list))
        .routes(routes!(tools::installed_remove))
        .routes(routes!(tools::installed_enable))
        .routes(routes!(tools::installed_disable))
        .routes(routes!(tools::skills_list))
        .routes(routes!(tools::skill_remove))
        .routes(routes!(tools::skill_enable))
        .routes(routes!(tools::skill_disable))
        .routes(routes!(tools::run_tool))
}

mod gateway_docs;

fn split() -> (Router<AppState>, utoipa::openapi::OpenApi) {
    let (router, mut doc) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .nest(BASE_PATH, api_routes())
        .split_for_parts();
    gateway_docs::merge(&mut doc);
    auth_responses(&mut doc);
    (router, doc)
}

/// Every operation behind the auth guard can answer 401 (no or unknown token) and 403 (the
/// token lacks the scope, or a browser origin / host that is not allowed): documented once
/// here instead of on each handler. `/health` (`security(())`) is public.
fn auth_responses(doc: &mut utoipa::openapi::OpenApi) {
    use utoipa::openapi::{ContentBuilder, Ref, RefOr, ResponseBuilder};
    let error = |description: &str| {
        RefOr::T(
            ResponseBuilder::new()
                .description(description)
                .content(
                    "application/json",
                    ContentBuilder::new()
                        .schema(Some(Ref::from_schema_name("ErrorBody")))
                        .build(),
                )
                .build(),
        )
    };
    for item in doc.paths.paths.values_mut() {
        for op in [
            &mut item.get,
            &mut item.put,
            &mut item.post,
            &mut item.delete,
            &mut item.patch,
        ]
        .into_iter()
        .flatten()
        {
            let public = op
                .security
                .as_ref()
                .is_some_and(|s| s.iter().all(|r| r == &Default::default()));
            if public {
                continue;
            }
            let responses = &mut op.responses.responses;
            responses.entry("401".into()).or_insert_with(|| {
                error("No token, or an unknown or revoked one (`api.unauthorized`)")
            });
            responses.entry("403".into()).or_insert_with(|| {
                error("The token lacks the scope (`api.insufficient_scope`), or the request comes from a browser origin or host that is not allowed (`api.forbidden_origin`, `api.forbidden_host`)")
            });
        }
    }
}

/// The OpenAPI document (paths under `/api/harness/v1`).
pub fn openapi_document() -> utoipa::openapi::OpenApi {
    split().1
}

/// The HTTP application for one listener.
pub fn router(state: AppState, cors_origins: &[String]) -> Router {
    let browser = state.browser.clone();
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
        .with_state(state.clone());
    let router = if let Some(browser) = &browser {
        crate::browser::static_routes(router, &browser.config)
    } else {
        router
    };
    let allowed: Arc<[String]> = cors_origins.to_vec().into();
    let origins: Vec<HeaderValue> = cors_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    let router = if origins.is_empty() {
        router
    } else {
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
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
                .expose_headers([header::HeaderName::from_static(
                    harness_core::control_api::API_VERSION_HEADER,
                )]),
        )
    };
    // Outermost, on every request including preflights: a foreign browser origin gets 403
    // before any route (and before CORS answers a preflight). Requests without an `Origin`
    // header (the CLI, SDKs, curl) are untouched.
    router
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::browser::guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            (allowed, state),
            origin_guard,
        ))
        // Outermost: every response (errors, 404s and refused origins included) names the
        // contract version.
        .layer(axum::middleware::map_response(api_version_header))
}

/// Adds `X-Harness-API-Version` to a response.
async fn api_version_header(mut res: axum::response::Response) -> axum::response::Response {
    res.headers_mut().insert(
        header::HeaderName::from_static(harness_core::control_api::API_VERSION_HEADER),
        HeaderValue::from(harness_core::control_api::API_VERSION),
    );
    res
}

/// `403 api.forbidden_origin` unless the request has no `Origin` or comes from the Harness
/// window or an allowed CORS origin (see [`harness_core::network::check_browser_origin`]).
async fn origin_guard(
    axum::extract::State((allowed, state)): axum::extract::State<(Arc<[String]>, AppState)>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let same_origin = state
        .browser
        .as_ref()
        .is_some_and(|b| b.same_origin(&state, req.headers(), false));
    if !same_origin
        && harness_core::network::check_browser_headers(
            req.headers().get("origin").map(|v| v.as_bytes()),
            req.headers().get("sec-fetch-site").map(|v| v.as_bytes()),
            &allowed,
        )
        .is_err()
    {
        return crate::error::ApiError::new(
            axum::http::StatusCode::FORBIDDEN,
            "api.forbidden_origin",
        )
        .into_response();
    }
    next.run(req).await
}
