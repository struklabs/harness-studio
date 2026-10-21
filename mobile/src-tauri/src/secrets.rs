//! Tokens and provider keys in the platform keychain (iOS Keychain, macOS Keychain for desktop
//! runs of the mobile UI). An entry is the key and the origin it belongs to; a request to any
//! other origin is refused, so a key typed for one provider can never be sent to another
//! server, even by a compromised page.

use serde::{Deserialize, Serialize};

const SERVICE: &str = "com.harnessstudio.mobile";
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

fn entry(account: &str) -> Result<keyring::Entry, String> {
    if !valid_account(account) {
        return Err("mobile.invalid_request".into());
    }
    keyring::Entry::new(SERVICE, account).map_err(|_| "mobile.keychain".to_string())
}

fn read(account: &str) -> Result<Option<Entry>, String> {
    match entry(account)?.get_password() {
        Ok(raw) => Ok(serde_json::from_str(&raw).ok()),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("mobile.keychain".into()),
    }
}

/// The key of `account` for a request to `origin`.
pub fn key_for(account: &str, origin: &str) -> Result<String, String> {
    match read(account)? {
        Some(entry) if entry.origin == origin => Ok(entry.key),
        Some(_) => Err("mobile.key_origin".into()),
        None => Err("mobile.key_missing".into()),
    }
}

/// Saves `key` for requests to `url`'s origin.
#[tauri::command]
pub fn secret_set(account: String, url: String, key: String) -> Result<(), String> {
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
    entry(&account)?
        .set_password(&raw)
        .map_err(|_| "mobile.keychain".to_string())
}

/// Whether `account` has a key (the key itself never goes back to the page).
#[tauri::command]
pub fn secret_has(account: String) -> Result<bool, String> {
    Ok(read(&account)?.is_some())
}

#[tauri::command]
pub fn secret_delete(account: String) -> Result<(), String> {
    match entry(&account)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err("mobile.keychain".into()),
    }
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
