//! `HEAD` / `POST /api/blobs/:digest`: Ollama's content-addressed upload used by `ollama create`
//! (the CLI uploads the GGUF named by `FROM ./model.gguf`, then sends its digest in `files`).
//!
//! Blobs live in `<primary models dir>/ollama-blobs/sha256-<hex>` (no extension, like
//! Ollama's own layout, so the Library does not list them as loose models). A blob is written
//! to `<name>.part-<id>` while it arrives and renamed once its SHA-256 matches the digest in
//! the URL; a mismatch deletes it. Model definitions made by `/api/create` point at these
//! files (see [`super::manage`]).

use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use harness_core::token_policy::GatewayEndpoint;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::error_response;
use crate::errors::GatewayError;
use crate::GwState;

/// Folder (under the primary models directory) that holds uploaded blobs.
pub const BLOB_DIR: &str = "ollama-blobs";

/// `sha256:<64 hex>` (or `sha256-<hex>`, the file-name form) -> lower-case hex.
pub fn parse_digest(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let hex = raw
        .strip_prefix("sha256:")
        .or_else(|| raw.strip_prefix("sha256-"))
        .ok_or_else(|| {
            format!(
                "invalid digest {:?}: expected sha256:<64 hex characters>",
                raw
            )
        })?;
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "invalid digest {:?}: expected sha256:<64 hex characters>",
            raw
        ));
    }
    Ok(hex.to_ascii_lowercase())
}

/// The blob directory of a models root.
pub fn blob_dir_in(models_dir: &Path) -> PathBuf {
    models_dir.join(BLOB_DIR)
}

/// Path of the blob `hex` inside `dir`.
pub fn blob_path_in(dir: &Path, hex: &str) -> PathBuf {
    dir.join(format!("sha256-{}", hex))
}

/// The blob directory of the primary models directory.
pub fn blob_dir() -> PathBuf {
    blob_dir_in(&harness_core::downloader::DownloadManager::models_dir())
}

/// Whether `path` is a file inside a blob directory (the only files `/api/delete` may remove).
pub fn is_blob_file(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .is_some_and(|d| d == BLOB_DIR)
        && path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.starts_with("sha256-") && !f.contains(".part"))
}

/// First four bytes are `GGUF`.
pub fn is_gguf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|_| &magic == b"GGUF")
        .unwrap_or(false)
}

fn auth(state: &GwState, headers: &HeaderMap) -> Result<crate::auth::Caller, GatewayError> {
    crate::auth::check(state, headers, false, GatewayEndpoint::Ollama)
}

fn digest_error(msg: String) -> Response {
    error_response(GatewayError::new(400, "invalid_digest", msg))
}

/// `HEAD /api/blobs/:digest`: 200 when the blob exists (the CLI skips the upload), else 404.
pub async fn head(
    State(state): State<GwState>,
    headers: HeaderMap,
    UrlPath(digest): UrlPath<String>,
) -> Response {
    if let Err(e) = auth(&state, &headers) {
        return error_response(e);
    }
    let hex = match parse_digest(&digest) {
        Ok(h) => h,
        Err(m) => return digest_error(m),
    };
    if blob_path_in(&blob_dir(), &hex).is_file() {
        StatusCode::OK.into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

/// `POST /api/blobs/:digest`: stores the request body as the blob, verifying its SHA-256.
pub async fn upload(
    State(state): State<GwState>,
    headers: HeaderMap,
    UrlPath(digest): UrlPath<String>,
    body: Body,
) -> Response {
    if let Err(e) = auth(&state, &headers) {
        return error_response(e);
    }
    let hex = match parse_digest(&digest) {
        Ok(h) => h,
        Err(m) => return digest_error(m),
    };
    match store(&blob_dir(), &hex, body).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => error_response(e),
    }
}

/// Streams `body` into `dir/sha256-<hex>` (skipped when it already exists).
async fn store(dir: &Path, hex: &str, body: Body) -> Result<(), GatewayError> {
    let io = |e: std::io::Error| {
        GatewayError::new(
            500,
            "blob_write_failed",
            format!("could not store the blob: {}", e),
        )
    };
    let target = blob_path_in(dir, hex);
    if target.is_file() {
        // Content-addressed: the same digest is the same file. Drain nothing and say so.
        return Ok(());
    }
    tokio::fs::create_dir_all(dir).await.map_err(io)?;
    let part = dir.join(format!(
        "sha256-{}.part-{}",
        hex,
        crate::proxy::request_id().replace(|c: char| !c.is_ascii_alphanumeric(), "")
    ));
    let result = write_verified(&part, hex, body).await;
    match result {
        Ok(()) => {
            if let Err(e) = tokio::fs::rename(&part, &target).await {
                // A concurrent upload of the same blob won the race: fine.
                let _ = tokio::fs::remove_file(&part).await;
                if !target.is_file() {
                    return Err(io(e));
                }
            }
            Ok(())
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
}

async fn write_verified(part: &Path, hex: &str, body: Body) -> Result<(), GatewayError> {
    let io = |e: std::io::Error| {
        GatewayError::new(
            500,
            "blob_write_failed",
            format!("could not store the blob: {}", e),
        )
    };
    let mut file = tokio::fs::File::create(part).await.map_err(io)?;
    let mut hasher = Sha256::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            GatewayError::new(400, "upload_interrupted", format!("upload failed: {}", e))
        })?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(io)?;
    }
    file.flush().await.map_err(io)?;
    drop(file);
    let got = to_hex(&hasher.finalize());
    if got != hex {
        return Err(GatewayError::new(
            400,
            "digest_mismatch",
            format!(
                "digest mismatch, expected \"sha256:{}\", got \"sha256:{}\"",
                hex, got
            ),
        ));
    }
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn digest_forms() {
        assert_eq!(parse_digest(&format!("sha256:{HEX}")).unwrap(), HEX);
        assert_eq!(parse_digest(&format!("sha256-{HEX}")).unwrap(), HEX);
        assert_eq!(
            parse_digest(&format!("sha256:{}", HEX.to_uppercase())).unwrap(),
            HEX
        );
        for bad in ["", "sha256:abc", "md5:abc", HEX, "sha256:../../etc/passwd"] {
            assert!(parse_digest(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn blob_file_detection() {
        let dir = blob_dir_in(Path::new("/models"));
        assert!(is_blob_file(&blob_path_in(&dir, HEX)));
        assert!(!is_blob_file(&dir.join(format!("sha256-{HEX}.part-1"))));
        assert!(!is_blob_file(Path::new("/models/org/repo/model.gguf")));
        assert!(!is_blob_file(&dir.join("notes.txt")));
    }

    #[tokio::test]
    async fn store_verifies_the_digest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(BLOB_DIR);
        // sha256("") is HEX.
        store(&dir, HEX, Body::from(Vec::<u8>::new()))
            .await
            .unwrap();
        assert!(blob_path_in(&dir, HEX).is_file());
        // Existing blob: a second upload is accepted without rewriting.
        store(&dir, HEX, Body::from("anything")).await.unwrap();
        assert_eq!(std::fs::metadata(blob_path_in(&dir, HEX)).unwrap().len(), 0);

        let other = "0".repeat(64);
        let e = store(&dir, &other, Body::from("not matching"))
            .await
            .unwrap_err();
        assert_eq!(e.status, 400);
        assert!(e.message.starts_with("digest mismatch"), "{}", e.message);
        assert!(!blob_path_in(&dir, &other).exists());
        // No temporary file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|f| f.file_name().to_string_lossy().contains(".part"))
            .collect();
        assert!(leftovers.is_empty());
    }
}
