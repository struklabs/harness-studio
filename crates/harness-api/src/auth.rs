//! Authentication, scopes and the Host check.
//!
//! * Every endpoint except `GET /health` needs `Authorization: Bearer <token>`: the session
//!   token (full access) or a named token (its scopes). `GET /openapi.json` is public while
//!   the API is bound to loopback (the schema holds no secrets and tools fetch it without
//!   credentials); on a network bind it needs `read`, so the API surface is not advertised to
//!   the LAN.
//! * The scope a request needs is decided here, from its method and route ([`required_scope`]).
//! * On a loopback bind the `Host` header must name the loopback address (DNS rebinding: a web
//!   page on a rebinding domain could otherwise reach the API "same origin"; it still would
//!   need a token, this closes the door earlier).

use axum::extract::{MatchedPath, Request, State};
use axum::http::{header, Method};
use axum::middleware::Next;
use axum::response::Response;
use harness_core::control_api::{self, Principal, Scope};

use crate::error::ApiError;
use crate::AppState;

/// The authenticated caller (request extension).
#[derive(Debug, Clone)]
pub struct Caller {
    pub principal: Principal,
    pub scopes: Vec<Scope>,
}

impl Caller {
    pub fn allows(&self, scope: Scope) -> bool {
        control_api::allows(&self.scopes, scope)
    }
}

/// Scope needed for a route (`route` = the matched path template without the base path).
/// `None` = public.
pub fn required_scope(method: &Method, route: &str, loopback: bool) -> Option<Scope> {
    match route {
        "/health" => return None,
        "/browser/login" | "/browser/session" => return None,
        "/openapi.json" if loopback => return None,
        // Any valid token may speak MCP; each tool checks its own scope.
        "/mcp" => return Some(Scope::Read),
        _ => {}
    }
    // Running a tool reads and writes files and runs programs; its log holds the arguments of
    // every call. Both are admin matters (listing the tools and reading the policy are `read`).
    // Running a tool is its own scope (`tools`): MCP clients and scripts may run the tools the
    // policy lets through. What would widen that (`approved`, a working directory of the
    // caller's choosing) is checked in the handler and needs admin.
    if *method == Method::POST && route == "/tools/{name}/run" {
        return Some(Scope::Tools);
    }
    // An MCP server's resources are its data, read the way a tool call reads them.
    if *method == Method::GET && route.starts_with("/tools/mcp/") {
        return Some(Scope::Tools);
    }
    if route == "/tools/automatic"
        || route.starts_with("/tools/automatic/")
        || route == "/tools/log"
        || (*method == Method::POST && route.starts_with("/tools/"))
        || (*method == Method::PUT && route == "/tools/search-provider")
    {
        return Some(Scope::Admin);
    }
    if route.starts_with("/browser/")
        || route == "/tokens"
        || route.starts_with("/tokens/")
        || route.ends_with("/exposure-ack")
        || route == "/shutdown"
    {
        return Some(Scope::Admin);
    }
    // Chats are private: their own scope for reading and compacting, not covered by `read`.
    if route == "/conversations" || route.starts_with("/conversations/") {
        return Some(Scope::Conversations);
    }
    if *method == Method::GET {
        return Some(Scope::Read);
    }
    // The narrow scopes: running servers (servers:control) and downloads (downloads:write).
    // The wide ones (servers:write, models:write) imply them.
    match (method.as_str(), route) {
        (
            "POST",
            "/servers/{id}/start"
            | "/servers/{id}/stop"
            | "/servers/{id}/retry"
            | "/servers/{id}/lora",
        )
        | ("POST", "/load" | "/unload") => return Some(Scope::ServersControl),
        ("POST", "/models/pull" | "/downloads/{id}/pause" | "/downloads/{id}/resume")
        | ("DELETE", "/downloads/{id}") => return Some(Scope::DownloadsWrite),
        // Safetensors to GGUF conversions are downloads that also compute; the environment
        // they run in (`/convert/env`) is an admin matter (the default below).
        ("POST", "/convert" | "/convert/{id}/cancel") | ("DELETE", "/convert/{id}") => {
            return Some(Scope::DownloadsWrite)
        }
        _ => {}
    }
    let first = route
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("");
    Some(match first {
        "models" | "downloads" | "definitions" | "presets" | "modelfile" => Scope::ModelsWrite,
        "servers" | "load" | "unload" | "benchmarks" => Scope::ServersWrite,
        _ => Scope::Admin,
    })
}

fn bearer(req: &Request) -> Option<&str> {
    let value = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

/// Verifies a presented token (constant-time for the session token; named tokens compare
/// salted hashes in constant time).
pub fn authenticate(state: &AppState, token: &str) -> Option<Caller> {
    if control_api::ct_eq(token.as_bytes(), state.session_token.as_bytes()) {
        return Some(Caller {
            principal: Principal::Session,
            scopes: vec![Scope::Admin],
        });
    }
    let (info, scopes) = state.tokens.verify(token)?;
    Some(Caller {
        principal: Principal::Named {
            id: info.id,
            name: info.name,
        },
        scopes,
    })
}

fn host_allowed(state: &AppState, req: &Request) -> bool {
    if !state.loopback {
        return true;
    }
    let Some(host) = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
    else {
        // HTTP/1.0 without Host: only local clients get here on a loopback bind.
        return true;
    };
    let port = state.addr.port();
    let host = host.to_ascii_lowercase();
    [
        format!("127.0.0.1:{}", port),
        format!("localhost:{}", port),
        format!("[::1]:{}", port),
    ]
    .contains(&host)
        || host == "127.0.0.1"
        || host == "localhost"
}

/// Route middleware: Host check, token and scope.
pub async fn guard(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    use axum::response::IntoResponse;
    if !host_allowed(&state, &req) {
        return ApiError::new(axum::http::StatusCode::FORBIDDEN, "api.forbidden_host")
            .into_response();
    }
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_string())
        .unwrap_or_default();
    let route = route
        .strip_prefix(control_api::BASE_PATH)
        .unwrap_or(&route)
        .to_string();
    let Some(needed) = required_scope(req.method(), &route, state.loopback) else {
        return next.run(req).await;
    };
    let caller = if req.headers().contains_key(header::AUTHORIZATION) {
        bearer(&req).and_then(|t| authenticate(&state, t))
    } else if let Some(browser) = &state.browser {
        match browser.caller(&state, req.headers(), req.method()) {
            Ok(caller) => Some(caller),
            Err(err) => return err.into_response(),
        }
    } else {
        None
    };
    let Some(caller) = caller else {
        let mut resp = ApiError::unauthorized().into_response();
        resp.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            header::HeaderValue::from_static("Bearer realm=\"harness\""),
        );
        return resp;
    };
    if !caller.allows(needed) {
        return ApiError::forbidden_scope(needed.as_str()).into_response();
    }
    req.extensions_mut().insert(caller);
    let cookie_auth = !req.headers().contains_key(header::AUTHORIZATION);
    let headers = req.headers().clone();
    let response = next.run(req).await;
    if cookie_auth {
        if let Some(browser) = state.browser {
            return crate::browser::protect_stream(response, browser, headers);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_table() {
        let get = Method::GET;
        let post = Method::POST;
        let del = Method::DELETE;
        assert_eq!(required_scope(&get, "/health", false), None);
        assert_eq!(required_scope(&get, "/openapi.json", true), None);
        assert_eq!(
            required_scope(&get, "/openapi.json", false),
            Some(Scope::Read)
        );
        assert_eq!(required_scope(&get, "/models", true), Some(Scope::Read));
        assert_eq!(required_scope(&get, "/tokens", true), Some(Scope::Admin));
        assert_eq!(required_scope(&get, "/modules", true), Some(Scope::Read));
        assert_eq!(
            required_scope(&post, "/modules/{id}", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&post, "/modules/{id}/cancel", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&del, "/modules/{id}", true),
            Some(Scope::Admin)
        );
        for path in ["/modules/{id}/action", "/modules/{id}/remove-data"] {
            assert_eq!(required_scope(&post, path, true), Some(Scope::Admin));
        }
        // Tools: listing and the policy are readable; running needs `tools`; changing the policy
        // and the log are admin.
        assert_eq!(required_scope(&get, "/tools", true), Some(Scope::Read));
        assert_eq!(
            required_scope(&get, "/tools/policy", true),
            Some(Scope::Read)
        );
        assert_eq!(required_scope(&get, "/tools/log", true), Some(Scope::Admin));
        // Installed tools, skills and the official registry: lists are `read`; installing,
        // switching and removing are `admin`.
        for path in ["/tools/registry", "/tools/installed", "/skills"] {
            assert_eq!(
                required_scope(&get, path, true),
                Some(Scope::Read),
                "{path}"
            );
        }
        for (method, path) in [
            (&post, "/tools/registry/install"),
            (&post, "/tools/installed/{name}/enable"),
            (&post, "/tools/installed/{name}/disable"),
            (&del, "/tools/installed/{name}"),
            (&post, "/skills/{name}/enable"),
            (&post, "/skills/{name}/disable"),
            (&del, "/skills/{name}"),
        ] {
            assert_eq!(
                required_scope(method, path, true),
                Some(Scope::Admin),
                "{path}"
            );
        }
        // MCP client: the state is `read`, resources are `tools`, signing in and out `admin`.
        assert_eq!(required_scope(&get, "/tools/mcp", true), Some(Scope::Read));
        assert_eq!(
            required_scope(&get, "/tools/mcp/{server}/resources", true),
            Some(Scope::Tools)
        );
        assert_eq!(
            required_scope(&post, "/tools/mcp/{server}/authorize", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&get, "/tools/search-provider", true),
            Some(Scope::Read)
        );
        assert_eq!(
            required_scope(&Method::PUT, "/tools/search-provider", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&post, "/tools/{name}/run", true),
            Some(Scope::Tools)
        );
        assert_eq!(
            required_scope(&Method::PUT, "/tools/policy", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&del, "/models/{id}", true),
            Some(Scope::ModelsWrite)
        );
        assert_eq!(
            required_scope(&post, "/models/pull", true),
            Some(Scope::DownloadsWrite)
        );
        assert_eq!(
            required_scope(&post, "/downloads/{id}/pause", true),
            Some(Scope::DownloadsWrite)
        );
        assert_eq!(
            required_scope(&del, "/downloads/{id}", true),
            Some(Scope::DownloadsWrite)
        );
        // Conversions are queued like downloads; the environment they run in is admin.
        for (method, route) in [
            (&post, "/convert"),
            (&post, "/convert/{id}/cancel"),
            (&del, "/convert/{id}"),
        ] {
            assert_eq!(
                required_scope(method, route, true),
                Some(Scope::DownloadsWrite),
                "{route}"
            );
        }
        assert_eq!(
            required_scope(&Method::GET, "/convert/plan", true),
            Some(Scope::Read)
        );
        assert_eq!(
            required_scope(&post, "/convert/env", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&del, "/convert/env", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&post, "/servers", true),
            Some(Scope::ServersWrite)
        );
        assert_eq!(
            required_scope(&del, "/servers/{id}", true),
            Some(Scope::ServersWrite)
        );
        assert_eq!(
            required_scope(&post, "/servers/{id}/stop", true),
            Some(Scope::ServersControl)
        );
        assert_eq!(
            required_scope(&post, "/servers/{id}/lora", true),
            Some(Scope::ServersControl)
        );
        assert_eq!(
            required_scope(&get, "/servers/{id}/lora", true),
            Some(Scope::Read)
        );
        assert_eq!(
            required_scope(&post, "/unload", true),
            Some(Scope::ServersControl)
        );
        assert_eq!(
            required_scope(&Method::PATCH, "/tokens/{id}", true),
            Some(Scope::Admin)
        );
        assert_eq!(required_scope(&get, "/otel", true), Some(Scope::Read));
        assert_eq!(
            required_scope(&Method::PUT, "/otel", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&post, "/otel/test", true),
            Some(Scope::Admin)
        );
        assert_eq!(
            required_scope(&post, "/definitions", true),
            Some(Scope::ModelsWrite)
        );
        assert_eq!(
            required_scope(&post, "/modelfile/import", true),
            Some(Scope::ModelsWrite)
        );
        assert_eq!(
            required_scope(&post, "/servers/{id}/start", true),
            Some(Scope::ServersControl)
        );
        assert_eq!(
            required_scope(&post, "/load", true),
            Some(Scope::ServersControl)
        );
        assert_eq!(
            required_scope(&post, "/benchmarks", true),
            Some(Scope::ServersWrite)
        );
        assert_eq!(
            required_scope(&post, "/servers/{id}/exposure-ack", true),
            Some(Scope::Admin)
        );
        assert_eq!(required_scope(&post, "/tokens", true), Some(Scope::Admin));
        assert_eq!(required_scope(&post, "/shutdown", true), Some(Scope::Admin));
        assert_eq!(required_scope(&post, "/mcp", true), Some(Scope::Read));
    }
}
