//! Duplicate model files.
//!
//! Fast pass: models with the same total size and the same "pre-hash" (SHA-256 over the part
//! sizes, the first MiB of the first part and the last MiB of the last part) are grouped.
//! Only models that share a size are read at all, and only 2 MiB each. "Tam doğrula" then
//! compares full SHA-256 digests (taken from `model_hashes.json` when the downloader
//! recorded one for the file, else computed).

use crate::models::LocalModel;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// Bytes read from each end of a file for the pre-hash.
pub const EDGE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateMember {
    pub model_id: String,
    pub name: String,
    pub path: std::path::PathBuf,
    pub origin: crate::domain::ModelOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub size_bytes: u64,
    pub pre_hash: String,
    pub members: Vec<DuplicateMember>,
}

/// Result of a full comparison of one group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedMember {
    pub model_id: String,
    pub sha256: String,
    /// Taken from `model_hashes.json` instead of reading the file.
    pub recorded: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Pre-hash of a model (see module docs).
pub fn pre_hash(model: &LocalModel) -> std::io::Result<String> {
    let mut h = Sha256::new();
    for p in &model.shard_paths {
        h.update(std::fs::metadata(p)?.len().to_le_bytes());
    }
    let (Some(first), Some(last)) = (model.shard_paths.first(), model.shard_paths.last()) else {
        return Err(std::io::Error::other("no files"));
    };
    let mut buf = vec![0u8; EDGE_BYTES as usize];
    let mut f = File::open(first)?;
    let n = read_up_to(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    let mut f = File::open(last)?;
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(EDGE_BYTES)))?;
    let n = read_up_to(&mut f, &mut buf)?;
    h.update(&buf[..n]);
    Ok(hex(&h.finalize()))
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = f.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// Groups of models that look identical (same size and pre-hash), largest first.
/// Incomplete split models are skipped.
pub fn find_duplicates(models: &[LocalModel]) -> Vec<DuplicateGroup> {
    let mut by_size: BTreeMap<(u64, usize), Vec<&LocalModel>> = BTreeMap::new();
    for m in models.iter().filter(|m| m.is_ready && m.size_bytes > 0) {
        by_size
            .entry((m.size_bytes, m.shard_paths.len()))
            .or_default()
            .push(m);
    }
    let mut groups = Vec::new();
    for ((size, _), same_size) in by_size.into_iter().rev() {
        if same_size.len() < 2 {
            continue;
        }
        let mut by_hash: BTreeMap<String, Vec<&LocalModel>> = BTreeMap::new();
        for m in same_size {
            if let Ok(h) = pre_hash(m) {
                by_hash.entry(h).or_default().push(m);
            }
        }
        for (pre_hash, ms) in by_hash {
            if ms.len() < 2 {
                continue;
            }
            groups.push(DuplicateGroup {
                size_bytes: size,
                pre_hash,
                members: ms
                    .into_iter()
                    .map(|m| DuplicateMember {
                        model_id: m.id.clone(),
                        name: m.name.clone(),
                        path: m.path.clone(),
                        origin: m.origin,
                    })
                    .collect(),
            });
        }
    }
    groups
}

/// Full SHA-256 of a model: the file's digest, or for split models the digest of the parts'
/// digests in order. `recorded` maps normalized paths (`library::norm`) to SHA-256.
pub fn full_hash(
    model: &LocalModel,
    recorded: &HashMap<String, String>,
) -> Result<(String, bool), String> {
    let mut parts = Vec::new();
    let mut all_recorded = true;
    for p in &model.shard_paths {
        match recorded.get(&super::norm(p)) {
            Some(h) => parts.push(h.to_lowercase()),
            None => {
                all_recorded = false;
                parts.push(crate::downloader::sha256_file(p)?);
            }
        }
    }
    if parts.len() == 1 {
        return Ok((parts.remove(0), all_recorded));
    }
    let mut h = Sha256::new();
    for p in &parts {
        h.update(p.as_bytes());
    }
    Ok((hex(&h.finalize()), all_recorded))
}

/// Full digests of the given models (a duplicate group). `recorded` is `model_hashes.json`
/// as stored (keys are paths in any separator style).
pub fn verify(
    models: &[LocalModel],
    recorded: &HashMap<String, String>,
) -> Result<Vec<VerifiedMember>, String> {
    let recorded: HashMap<String, String> = recorded
        .iter()
        .map(|(k, v)| (super::norm(std::path::Path::new(k)), v.clone()))
        .collect();
    let recorded = &recorded;
    models
        .iter()
        .map(|m| {
            let (sha256, from_record) = full_hash(m, recorded)?;
            Ok(VerifiedMember {
                model_id: m.id.clone(),
                sha256,
                recorded: from_record,
            })
        })
        .collect()
}
