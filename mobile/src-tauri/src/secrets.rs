//! Tokens and provider keys in the platform's secret store: the iOS / macOS Keychain
//! (`keyring`), and on Android a key in the Android Keystore that encrypts them (the
//! `SecureStore` Kotlin plugin in `gen/android`). An entry is the key and the origin it belongs
//! to; a request to any other origin is refused, so a key typed for one provider can never be
//! sent to another server, even by a compromised page.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};

const KEY_LIMIT: usize = 8 * 1024;

#[derive(Serialize, Deserialize)]
struct Entry {
    origin: String,
    key: String,
}

fn valid_account(account: &str) -> bool {
    !account.is_empty()
        && account.len() <= 128
        && account
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

fn checked(account: &str) -> Result<&str, String> {
    valid_account(account)
        .then_some(account)
        .ok_or_else(|| "mobile.invalid_request".to_string())
}

#[cfg(not(target_os = "android"))]
mod store {
    const SERVICE: &str = "com.harnessstudio.mobile";

    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|_| "mobile.keychain".to_string())
    }

    pub fn get<R: tauri::Runtime>(
        _: &tauri::AppHandle<R>,
        account: &str,
    ) -> Result<Option<String>, String> {
        match entry(account)?.get_password() {
            Ok(raw) => Ok(Some(raw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("mobile.keychain".into()),
        }
    }

    pub fn set<R: tauri::Runtime>(
        _: &tauri::AppHandle<R>,
        account: &str,
        value: &str,
    ) -> Result<(), String> {
        entry(account)?
            .set_password(value)
            .map_err(|_| "mobile.keychain".to_string())
    }

    pub fn delete<R: tauri::Runtime>(_: &tauri::AppHandle<R>, account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("mobile.keychain".into()),
        }
    }

    pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
        tauri::plugin::Builder::new("secure-store").build()
    }
}

#[cfg(target_os = "android")]
mod store {
    use serde::Deserialize;
    use tauri::{plugin::PluginHandle, Manager};

    pub struct Handle<R: tauri::Runtime>(PluginHandle<R>);

    #[derive(Deserialize)]
    struct Value {
        value: Option<String>,
    }

    fn handle<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<&PluginHandle<R>, String> {
        app.try_state::<Handle<R>>()
            .map(|h| &h.inner().0)
            .ok_or_else(|| "mobile.keychain".to_string())
    }

    pub fn get<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        account: &str,
    ) -> Result<Option<String>, String> {
        let v: Value = handle(app)?
            .run_mobile_plugin("get", serde_json::json!({ "account": account }))
            .map_err(|_| "mobile.keychain".to_string())?;
        Ok(v.value)
    }

    pub fn set<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        account: &str,
        value: &str,
    ) -> Result<(), String> {
        handle(app)?
            .run_mobile_plugin::<serde_json::Value>(
                "set",
                serde_json::json!({ "account": account, "value": value }),
            )
            .map(|_| ())
            .map_err(|_| "mobile.keychain".to_string())
    }

    pub fn delete<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        account: &str,
    ) -> Result<(), String> {
        handle(app)?
            .run_mobile_plugin::<serde_json::Value>(
                "delete",
                serde_json::json!({ "account": account }),
            )
            .map(|_| ())
            .map_err(|_| "mobile.keychain".to_string())
    }

    pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
        tauri::plugin::Builder::new("secure-store")
            .setup(|app, api| {
                let handle =
                    api.register_android_plugin("com.harnessstudio.mobile", "SecureStorePlugin")?;
                app.manage(Handle(handle));
                Ok(())
            })
            .build()
    }
}

/// The plugin that sets up the platform's store (Android only; a no-op elsewhere).
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    store::plugin()
}

fn read<R: Runtime>(app: &AppHandle<R>, account: &str) -> Result<Option<Entry>, String> {
    Ok(store::get(app, checked(account)?)?.and_then(|raw| serde_json::from_str(&raw).ok()))
}

/// The key of `account` for a request to `origin`.
pub fn key_for<R: Runtime>(
    app: &AppHandle<R>,
    account: &str,
    origin: &str,
) -> Result<String, String> {
    match read(app, account)? {
        Some(entry) if entry.origin == origin => Ok(entry.key),
        Some(_) => Err("mobile.key_origin".into()),
        None => Err("mobile.key_missing".into()),
    }
}

/// Saves `key` for requests to `url`'s origin.
#[tauri::command]
pub fn secret_set<R: Runtime>(
    app: AppHandle<R>,
    account: String,
    url: String,
    key: String,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.len() > KEY_LIMIT || key.chars().any(char::is_control) {
        return Err("mobile.invalid_key".into());
    }
    let (_, origin) = super::origin_of(&url)?;
    let raw = serde_json::to_string(&Entry {
        origin,
        key: key.to_string(),
    })
    .map_err(|e| e.to_string())?;
    store::set(&app, checked(&account)?, &raw)
}

/// Whether `account` has a key (the key itself never goes back to the page).
#[tauri::command]
pub fn secret_has<R: Runtime>(app: AppHandle<R>, account: String) -> Result<bool, String> {
    Ok(read(&app, &account)?.is_some())
}

#[tauri::command]
pub fn secret_delete<R: Runtime>(app: AppHandle<R>, account: String) -> Result<(), String> {
    store::delete(&app, checked(&account)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_plain_names() {
        assert!(valid_account("harness:home"));
        assert!(valid_account("provider:openai-1"));
        for bad in ["", "a/b", "a b", "x\n", &"a".repeat(129)] {
            assert!(!valid_account(bad), "{bad:?}");
        }
    }
}
