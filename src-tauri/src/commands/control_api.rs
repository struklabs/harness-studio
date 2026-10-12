// Settings > Developer API: the Harness Control API (harness-api) and its tokens.

/// Ownership of the data directory (`instance.lock`), decided at startup.
pub(crate) struct InstanceState {
    pub(crate) lock: Option<Arc<crate::instance::InstanceLock>>,
    /// The other owner, when this GUI is not the owner.
    other: Option<crate::instance::InstanceInfo>,
    error: Option<String>,
}

impl InstanceState {
    pub(crate) fn owner(lock: Arc<crate::instance::InstanceLock>) -> Self {
        Self {
            lock: Some(lock),
            other: None,
            error: None,
        }
    }

    pub(crate) fn not_owner(e: crate::instance::AcquireError) -> Self {
        let other = match &e {
            crate::instance::AcquireError::Held { owner } => owner.clone(),
            crate::instance::AcquireError::Io(_) => None,
        };
        Self {
            lock: None,
            other,
            error: Some(e.to_string()),
        }
    }
}

/// The API listener and the services it serves (same instances as the GUI's).
pub(crate) struct ControlApiState {
    pub(crate) server: Arc<harness_api::ApiServer>,
    pub(crate) ctx: harness_api::ApiContext,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ControlApiOverview {
    settings: crate::control_api::ControlApiSettings,
    status: harness_api::ApiStatus,
    /// This GUI owns the data directory (only the owner serves the API).
    owner: bool,
    other_owner: Option<crate::instance::InstanceInfo>,
    owner_error: Option<String>,
    /// `.../openapi.json` while running.
    openapi_url: Option<String>,
    named_tokens: usize,
}

async fn control_api_overview(
    instance: &InstanceState,
    api: &ControlApiState,
) -> Result<ControlApiOverview, String> {
    let status = api.server.status().await;
    let named_tokens = blocking(|| crate::control_api::TokenStore::global().count()).await?;
    Ok(ControlApiOverview {
        settings: AppSettings::load().control_api,
        openapi_url: status.base_url.as_ref().map(|b| format!("{}/openapi.json", b)),
        status,
        owner: instance.lock.is_some(),
        other_owner: instance.other.clone(),
        owner_error: instance.error.clone(),
        named_tokens,
    })
}

#[tauri::command]
pub(crate) async fn get_control_api_status(
    instance: State<'_, InstanceState>,
    api: State<'_, ControlApiState>,
) -> Result<ControlApiOverview, String> {
    control_api_overview(&instance, &api).await
}

/// Validates and saves the API settings, then starts, restarts or stops the listener.
/// Errors are i18n keys (`api.*`, `network.invalid_host`).
#[tauri::command]
pub(crate) async fn set_control_api_settings(
    instance: State<'_, InstanceState>,
    api: State<'_, ControlApiState>,
    settings: crate::control_api::ControlApiSettings,
) -> Result<ControlApiOverview, String> {
    let validated = blocking(move || {
        let count = crate::control_api::TokenStore::global().count();
        crate::control_api::validate_settings(&settings, count)
    })
    .await??;
    if validated.enabled && instance.lock.is_none() {
        return Err("api.not_owner".to_string());
    }
    {
        let validated = validated.clone();
        blocking(move || {
            let mut all = AppSettings::load();
            all.control_api = validated;
            all.save()
        })
        .await??;
    }
    if validated.enabled {
        api.server.start(api.ctx.clone(), &validated).await?;
    } else {
        api.server.stop().await;
    }
    control_api_overview(&instance, &api).await
}

/// The current session token (Settings shows it masked; copy/reveal on request).
#[tauri::command]
pub(crate) async fn get_control_api_token(api: State<'_, ControlApiState>) -> Result<Option<String>, String> {
    Ok(api.server.session_token().await)
}

#[tauri::command]
pub(crate) async fn list_api_tokens() -> Result<Vec<crate::control_api::TokenInfo>, String> {
    blocking(|| crate::control_api::TokenStore::global().list()).await?
}

/// Creates a named token (with its gateway policy: allowed models and endpoints, rate
/// limits); its value is returned once and never stored.
#[tauri::command]
pub(crate) async fn create_api_token(
    name: String,
    scopes: Vec<crate::control_api::Scope>,
    policy: Option<crate::control_api::TokenPolicy>,
) -> Result<crate::control_api::CreatedToken, String> {
    blocking(move || {
        crate::control_api::TokenStore::global().create_with_policy(
            &name,
            &scopes,
            &policy.unwrap_or_default(),
        )
    })
    .await?
}

/// Changes a token's name, scopes or gateway policy; the next request uses the new values.
#[tauri::command]
pub(crate) async fn update_api_token(
    id: String,
    change: crate::control_api::TokenUpdate,
) -> Result<crate::control_api::TokenInfo, String> {
    blocking(move || crate::control_api::TokenStore::global().update(&id, &change)).await?
}

#[tauri::command]
pub(crate) async fn revoke_api_token(id: String) -> Result<(), String> {
    blocking(move || crate::control_api::TokenStore::global().revoke(&id)).await?
}
