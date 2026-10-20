//! Serve-only administration. Privileged actions have a fixed, root-owned helper;
//! backups are authenticated encryption, never a recursive filesystem archive.
use crate::{
    error::{blocking, ApiError, ApiResult, Body},
    AppState,
};
use axum::{extract::State, http::StatusCode, Json};
use harness_core::control_api::TokenInfo;
use ring::aead;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path, time::Duration};

const FILES: &[&str] = &[
    "settings.json",
    "model_definitions.json",
    "server_profiles.json",
    "inference_presets.json",
];
const LIMIT: usize = 256 * 1024;
const HELPER: &str = "/usr/lib/harness/server-control";

fn enabled(s: &AppState) -> ApiResult<()> {
    if s.browser.is_none() {
        return Err(ApiError::not_found("browser", "disabled"));
    }
    Ok(())
}
fn managed() -> bool {
    cfg!(target_os = "linux") && std::env::var("HARNESS_SERVICE").as_deref() == Ok("systemd")
}
fn invalid() -> ApiError {
    ApiError::bad_request("browser.invalid_backup")
}

#[derive(Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EncryptedBackup {
    format: u32,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    files: BTreeMap<String, Value>,
    tokens: Vec<TokenInfo>,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupRequest {
    password: String,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreRequest {
    password: String,
    archive: EncryptedBackup,
}

fn key(password: &str, salt: &[u8]) -> ApiResult<aead::LessSafeKey> {
    if !(12..=1024).contains(&password.len()) || salt.len() != 16 {
        return Err(invalid());
    }
    let mut bytes = [0; 32];
    argon2::Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut bytes)
        .map_err(|_| invalid())?;
    let key = aead::UnboundKey::new(&aead::CHACHA20_POLY1305, &bytes).map_err(|_| invalid())?;
    bytes.fill(0);
    Ok(aead::LessSafeKey::new(key))
}
fn encrypt(snapshot: &Snapshot, password: &str) -> ApiResult<EncryptedBackup> {
    let mut salt = vec![0; 16];
    let mut nonce = [0; 12];
    getrandom::fill(&mut salt).map_err(|_| invalid())?;
    getrandom::fill(&mut nonce).map_err(|_| invalid())?;
    let mut ciphertext = serde_json::to_vec(snapshot).map_err(|_| invalid())?;
    if ciphertext.len() > LIMIT {
        return Err(invalid());
    }
    key(password, &salt)?
        .seal_in_place_append_tag(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(b"harness-backup-v1"),
            &mut ciphertext,
        )
        .map_err(|_| invalid())?;
    Ok(EncryptedBackup {
        format: 1,
        salt,
        nonce: nonce.to_vec(),
        ciphertext,
    })
}
fn decrypt(mut archive: EncryptedBackup, password: &str) -> ApiResult<Snapshot> {
    if archive.format != 1 || archive.ciphertext.len() > LIMIT + 16 {
        return Err(invalid());
    }
    let nonce: [u8; 12] = archive.nonce.try_into().map_err(|_| invalid())?;
    let key = key(password, &archive.salt)?;
    let plain = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(b"harness-backup-v1"),
            &mut archive.ciphertext,
        )
        .map_err(|_| invalid())?;
    let result = serde_json::from_slice(plain).map_err(|_| invalid());
    plain.fill(0);
    result
}
fn regular(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => Ok(true),
        Ok(_) => Err("browser.invalid_backup".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
fn validate(snapshot: &Snapshot) -> Result<(), String> {
    for (name, value) in &snapshot.files {
        if !FILES.contains(&name.as_str()) || !value.is_object() {
            return Err("browser.invalid_backup".into());
        }
        // Validate typed documents and exact supported schema before touching the live directory.
        let version = value["schema_version"].as_u64();
        let valid = match name.as_str() {
            "settings.json" => {
                version == Some(3)
                    && serde_json::from_value::<harness_core::config::AppSettings>(value.clone())
                        .is_ok()
            }
            "model_definitions.json" => {
                version == Some(3)
                    && serde_json::from_value::<harness_core::domain::store::DefinitionsFile>(
                        value.clone(),
                    )
                    .is_ok()
            }
            "server_profiles.json" => {
                version == Some(2)
                    && serde_json::from_value::<harness_core::domain::store::ProfilesFile>(
                        value.clone(),
                    )
                    .is_ok()
            }
            "inference_presets.json" => {
                version == Some(1)
                    && serde_json::from_value::<harness_core::domain::store::PresetsFile>(
                        value.clone(),
                    )
                    .is_ok()
            }
            _ => false,
        };
        if !valid {
            return Err("browser.invalid_backup".into());
        }
    }
    Ok(())
}

#[utoipa::path(post, path = "/browser/backup", tag = "system", request_body = BackupRequest, responses((status = 200, body = EncryptedBackup)))]
pub async fn backup(
    State(s): State<AppState>,
    Body(req): Body<BackupRequest>,
) -> ApiResult<Json<EncryptedBackup>> {
    enabled(&s)?;
    // One expensive password operation at a time, shared with restore.
    let _guard = backup_gate()
        .try_lock()
        .map_err(|_| ApiError::conflict("browser.operation_busy"))?;
    let archive = blocking(move || {
        let dir = s.ctx.data_dir.join("config");
        let mut files = BTreeMap::new();
        for name in FILES {
            let path = dir.join(name);
            if regular(&path)? {
                if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > LIMIT as u64 {
                    return Err("browser.invalid_backup".into());
                }
                let value =
                    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                files.insert((*name).into(), value);
            }
        }
        let snapshot = Snapshot {
            files,
            tokens: s.tokens.list()?,
        };
        validate(&snapshot)?;
        encrypt(&snapshot, &req.password).map_err(|e| e.code)
    })
    .await??;
    Ok(Json(archive))
}
fn backup_gate() -> &'static tokio::sync::Mutex<()> {
    static GATE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    &GATE
}

#[utoipa::path(post, path = "/browser/restore", tag = "system", request_body = RestoreRequest, responses((status = 202)))]
pub async fn restore(
    State(s): State<AppState>,
    Body(req): Body<RestoreRequest>,
) -> ApiResult<StatusCode> {
    enabled(&s)?;
    let _guard = backup_gate()
        .try_lock()
        .map_err(|_| ApiError::conflict("browser.operation_busy"))?;
    blocking(move || {
        let snapshot = decrypt(req.archive, &req.password).map_err(|e| e.code)?;
        validate(&snapshot)?;
        let path = s.ctx.data_dir.join("restore-pending.json");
        if path.exists() {
            return Err("browser.restore_pending".into());
        }
        harness_core::instance::write_private_file(
            &path,
            &serde_json::to_vec(&snapshot).map_err(|e| e.to_string())?,
        )
    })
    .await??;
    Ok(StatusCode::ACCEPTED)
}

/// Called only after acquiring the instance lock, before opening services. A private rollback
/// journal makes an interrupted restore retryable; credentials and model files are untouched.
pub fn apply_pending(dir: &Path) -> Result<(), String> {
    let pending = dir.join("restore-pending.json");
    if !regular(&pending)? {
        return Ok(());
    }
    let snapshot: Snapshot =
        serde_json::from_slice(&std::fs::read(&pending).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    validate(&snapshot)?;
    let config = dir.join("config");
    std::fs::create_dir_all(&config).map_err(|e| e.to_string())?;
    let rollback = dir.join("restore-rollback.json");
    if !rollback.exists() {
        let mut previous = BTreeMap::<String, Option<Vec<u8>>>::new();
        for name in snapshot
            .files
            .keys()
            .map(String::as_str)
            .chain(std::iter::once("restored-token-metadata.json"))
        {
            let path = config.join(name);
            let bytes = if regular(&path)? {
                Some(std::fs::read(path).map_err(|e| e.to_string())?)
            } else {
                None
            };
            previous.insert(name.into(), bytes);
        }
        harness_core::instance::write_private_file(
            &rollback,
            &serde_json::to_vec(&previous).map_err(|e| e.to_string())?,
        )?;
    }
    let result = (|| {
        for (name, value) in &snapshot.files {
            regular(&config.join(name))?;
            harness_core::instance::write_private_file(
                &config.join(name),
                &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
            )?;
        }
        harness_core::instance::write_private_file(
            &config.join("restored-token-metadata.json"),
            &serde_json::to_vec(&snapshot.tokens).map_err(|e| e.to_string())?,
        )?;
        Ok::<_, String>(())
    })();
    if let Err(e) = result {
        let previous: BTreeMap<String, Option<Vec<u8>>> =
            serde_json::from_slice(&std::fs::read(&rollback).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        for (name, bytes) in previous {
            if !FILES.contains(&name.as_str()) && name != "restored-token-metadata.json" {
                return Err("browser.invalid_backup".into());
            }
            match bytes {
                Some(bytes) => {
                    harness_core::instance::write_private_file(&config.join(name), &bytes)?
                }
                None => {
                    let _ = std::fs::remove_file(config.join(name));
                }
            }
        }
        return Err(e);
    }
    std::fs::remove_file(pending).map_err(|e| e.to_string())?;
    std::fs::remove_file(rollback).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct ServiceStatus {
    managed: bool,
    uptime_seconds: u64,
    start_on_boot: bool,
    version: String,
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ServiceAction {
    Restart,
    Enable,
    Disable,
}
async fn helper(args: &[&str]) -> ApiResult<String> {
    if !managed() {
        return Err(ApiError::conflict("browser.unmanaged_service"));
    }
    let output = tokio::time::timeout(
        Duration::from_secs(180),
        tokio::process::Command::new("sudo")
            .args(["-n", HELPER])
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| ApiError::conflict("browser.operation_failed"))?
    .map_err(|_| ApiError::conflict("browser.operation_failed"))?;
    if !output.status.success() {
        return Err(ApiError::conflict("browser.operation_failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
#[utoipa::path(get, path = "/browser/service", tag = "system", responses((status = 200, body = ServiceStatus)))]
pub async fn service_status(State(s): State<AppState>) -> ApiResult<Json<ServiceStatus>> {
    enabled(&s)?;
    let uptime_seconds = s.browser.as_ref().unwrap().started.elapsed().as_secs();
    let start_on_boot = if managed() {
        helper(&["is-enabled"]).await? == "enabled\n"
    } else {
        false
    };
    Ok(Json(ServiceStatus {
        managed: managed(),
        uptime_seconds,
        start_on_boot,
        version: s.ctx.version,
    }))
}
#[utoipa::path(post, path = "/browser/service", tag = "system", request_body = ServiceAction, responses((status = 202)))]
pub async fn service_action(
    State(s): State<AppState>,
    Body(action): Body<ServiceAction>,
) -> ApiResult<StatusCode> {
    enabled(&s)?;
    helper(&[match action {
        ServiceAction::Restart => "restart",
        ServiceAction::Enable => "enable",
        ServiceAction::Disable => "disable",
    }])
    .await?;
    Ok(StatusCode::ACCEPTED)
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct ServiceLogs {
    text: String,
}
#[utoipa::path(get, path = "/browser/logs", tag = "system", responses((status = 200, body = ServiceLogs)))]
pub async fn logs(State(s): State<AppState>) -> ApiResult<Json<ServiceLogs>> {
    enabled(&s)?;
    Ok(Json(ServiceLogs {
        text: helper(&["logs"]).await?,
    }))
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    digest: Option<String>,
}
#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Serialize, utoipa::ToSchema)]
pub struct UpdateStatus {
    current: String,
    version: String,
    available: bool,
    applicable: bool,
}
fn release_status(release: &Release, current: &str, architecture: &str) -> ApiResult<UpdateStatus> {
    let version = release
        .tag_name
        .strip_prefix('v')
        .ok_or_else(|| ApiError::bad_request("browser.update_failed"))?;
    let latest = semver::Version::parse(version)
        .map_err(|_| ApiError::bad_request("browser.update_failed"))?;
    let current_version = semver::Version::parse(current)
        .map_err(|_| ApiError::bad_request("browser.update_failed"))?;
    let name = format!("harness-server-{version}-linux-{architecture}.deb");
    let expected =
        format!("https://github.com/struklabs/harness-studio/releases/download/v{version}/{name}");
    let available =
        !release.draft && !release.prerelease && latest.pre.is_empty() && latest > current_version;
    let applicable = available
        && release.assets.iter().any(|a| {
            a.name == name
                && a.browser_download_url == expected
                && a.digest.as_ref().is_some_and(|d| {
                    d.strip_prefix("sha256:").is_some_and(|hex| {
                        hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                })
        });
    Ok(UpdateStatus {
        current: current.into(),
        version: version.into(),
        available,
        applicable,
    })
}
async fn check(current: &str) -> ApiResult<UpdateStatus> {
    let release: Release = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("HarnessStudio")
        .build()
        .map_err(|_| ApiError::bad_request("browser.update_failed"))?
        .get("https://api.github.com/repos/struklabs/harness-studio/releases/latest")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|_| ApiError::bad_request("browser.update_failed"))?
        .json()
        .await
        .map_err(|_| ApiError::bad_request("browser.update_failed"))?;
    release_status(&release, current, std::env::consts::ARCH)
}
#[utoipa::path(get, path = "/browser/update", tag = "system", responses((status = 200, body = UpdateStatus)))]
pub async fn update_check(State(s): State<AppState>) -> ApiResult<Json<UpdateStatus>> {
    enabled(&s)?;
    let mut status = check(&s.ctx.version).await?;
    status.applicable &= managed();
    Ok(Json(status))
}
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    version: String,
}
#[utoipa::path(post, path = "/browser/update", tag = "system", request_body = UpdateRequest, responses((status = 202)))]
pub async fn update_apply(
    State(s): State<AppState>,
    Body(req): Body<UpdateRequest>,
) -> ApiResult<StatusCode> {
    enabled(&s)?;
    let status = check(&s.ctx.version).await?;
    if !status.applicable || status.version != req.version {
        return Err(ApiError::conflict("browser.update_failed"));
    }
    helper(&["update", &req.version]).await?;
    Ok(StatusCode::ACCEPTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encrypted_backup_roundtrip_tamper_password_and_limits() {
        let snapshot = Snapshot {
            files: BTreeMap::new(),
            tokens: vec![],
        };
        let archive = encrypt(&snapshot, "backup-password-1234").unwrap();
        let bytes = serde_json::to_vec(&archive).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("files"));
        assert!(decrypt(serde_json::from_slice(&bytes).unwrap(), "wrong-password").is_err());
        assert!(decrypt(
            serde_json::from_slice(&bytes).unwrap(),
            "backup-password-1234"
        )
        .unwrap()
        .files
        .is_empty());
        let mut archive: EncryptedBackup = serde_json::from_slice(&bytes).unwrap();
        archive.ciphertext[0] ^= 1;
        assert!(decrypt(archive, "backup-password-1234").is_err());
        assert!(encrypt(&snapshot, "short").is_err());
    }
    #[test]
    fn restore_allowlist_preserves_models_and_credentials() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("model.gguf"), "weights").unwrap();
        std::fs::write(
            dir.path().join("config/browser-admin.json"),
            "password hash",
        )
        .unwrap();
        let mut files = BTreeMap::new();
        files.insert(
            "settings.json".into(),
            serde_json::json!({"schema_version":3,"server":{"default_ngl":17}}),
        );
        let mut snapshot = Snapshot {
            files,
            tokens: vec![],
        };
        validate(&snapshot).unwrap();
        std::fs::write(
            dir.path().join("restore-pending.json"),
            serde_json::to_vec(&snapshot).unwrap(),
        )
        .unwrap();
        apply_pending(dir.path()).unwrap();
        assert!(!dir.path().join("restore-pending.json").exists());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("model.gguf")).unwrap(),
            "weights"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config/browser-admin.json")).unwrap(),
            "password hash"
        );
        snapshot
            .files
            .insert("../api-token".into(), serde_json::json!({}));
        assert!(validate(&snapshot).is_err());
        snapshot.files.remove("../api-token");
        snapshot.files.get_mut("settings.json").unwrap()["schema_version"] = 999.into();
        assert!(validate(&snapshot).is_err());
    }
    #[test]
    fn update_requires_new_stable_release_and_official_digest_asset() {
        let version = "0.2.0";
        let mut release = Release { tag_name: format!("v{version}"), draft: false, prerelease: false, assets: vec![Asset { name: format!("harness-server-{version}-linux-x86_64.deb"), browser_download_url: format!("https://github.com/struklabs/harness-studio/releases/download/v{version}/harness-server-{version}-linux-x86_64.deb"), digest: Some(format!("sha256:{}", "a".repeat(64))) }] };
        assert!(
            release_status(&release, "0.1.2", "x86_64")
                .unwrap()
                .applicable
        );
        assert!(
            !release_status(&release, "0.2.0", "x86_64")
                .unwrap()
                .available
        );
        assert!(
            !release_status(&release, "0.1.2", "aarch64")
                .unwrap()
                .applicable
        );
        release.assets[0].digest = None;
        assert!(
            !release_status(&release, "0.1.2", "x86_64")
                .unwrap()
                .applicable
        );
        release.prerelease = true;
        assert!(
            !release_status(&release, "0.1.2", "x86_64")
                .unwrap()
                .available
        );
        release.tag_name = "invalid".into();
        assert!(release_status(&release, "0.1.2", "x86_64").is_err());
    }
}
