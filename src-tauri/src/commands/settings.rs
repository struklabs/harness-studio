#[tauri::command]
pub(crate) fn get_app_settings() -> AppSettings {
    AppSettings::load()
}

/// Write-only: stores a new Hugging Face token in the OS keychain. The token is never
/// returned to the frontend (settings only carry `hf_token_set`).
#[tauri::command]
pub(crate) async fn set_hf_token(token: String) -> Result<AppSettings, String> {
    blocking(move || {
        AppSettings::set_hf_token(&token)?;
        Ok(AppSettings::load())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn clear_hf_token() -> Result<AppSettings, String> {
    blocking(|| {
        AppSettings::clear_hf_token()?;
        Ok(AppSettings::load())
    })
    .await?
}

// --- Sign in with Hugging Face (OAuth PKCE; see harness_core::hf_auth) ---

/// Sign-in state for Settings: kind (none / token / oauth), cached profile, client id. Never a
/// token.
#[tauri::command]
pub(crate) async fn hf_auth_status() -> Result<harness_core::hf_auth::HfAuthStatus, String> {
    blocking(harness_core::hf_auth::status).await?
}

/// Opens the system browser on huggingface.co and waits (loopback redirect on 127.0.0.1) until
/// the user approves, declines, cancels ([`hf_oauth_cancel`]) or five minutes pass.
#[tauri::command]
pub(crate) async fn hf_oauth_login(
    app: tauri::AppHandle,
) -> Result<harness_core::hf_auth::HfAuthStatus, String> {
    use tauri_plugin_opener::OpenerExt;
    let settings = blocking(AppSettings::load).await?;
    let Some((client_id, _)) = harness_core::hf_auth::client_id(&settings) else {
        return Err(harness_core::error_key::err("hf_auth.no_client_id").into());
    };
    harness_core::hf_auth::login_with_browser(&client_id, |url| {
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub(crate) fn hf_oauth_cancel() -> bool {
    harness_core::hf_auth::cancel_login()
}

/// The account of the current token (OAuth or pasted), checked online.
#[tauri::command]
pub(crate) async fn hf_whoami() -> Result<harness_core::hf_auth::HfUser, String> {
    match harness_core::hf_auth::current_token().await {
        Some(token) => harness_core::hf_auth::whoami(&token).await,
        None => Err(harness_core::error_key::err("hf_auth.not_logged_in").into()),
    }
}

/// Signs out: deletes the OAuth session and the token from the keychain.
#[tauri::command]
pub(crate) async fn hf_logout() -> Result<AppSettings, String> {
    blocking(|| {
        harness_core::hf_auth::logout()?;
        Ok(AppSettings::load())
    })
    .await?
}

/// Sets (or clears, with an empty string) the OAuth app client id in Settings.
#[tauri::command]
pub(crate) async fn set_hf_oauth_client_id(
    client_id: String,
) -> Result<harness_core::hf_auth::HfAuthStatus, String> {
    blocking(move || {
        let client_id = client_id.trim().to_string();
        if !client_id.is_empty() && !harness_core::hf_auth::valid_client_id(&client_id) {
            return Err(harness_core::error_key::err("hf_auth.invalid_client_id").into());
        }
        let mut settings = AppSettings::load();
        settings.hf_oauth_client_id = client_id;
        settings.save()?;
        harness_core::hf_auth::status()
    })
    .await?
}
