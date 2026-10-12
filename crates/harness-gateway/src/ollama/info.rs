//! What `/api/tags`, `/api/ps` and `/api/show` report about a model definition's GGUF:
//! Ollama `details`, `model_info` (the GGUF key/values), the chat template and capabilities.
//!
//! Header facts come from `harness_core::gguf::read_gguf_metadata` (cached per file and
//! mtime); `/api/show` also reads every key/value here. Large arrays (`tokenizer.ggml.tokens`,
//! `merges`...) are returned empty unless `verbose` is set, like Ollama does.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use harness_core::domain::{detect_kind, ModelDefinition, ModelKind};
use harness_core::gguf::GgufMetadata;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::convert;

/// Facts of one GGUF file.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub meta: Option<GgufMetadata>,
    pub kind: ModelKind,
    /// Bytes of all shards.
    pub size: u64,
    pub modified: SystemTime,
    /// SHA-256 recorded when Harness downloaded the file, else a hash of path, size and mtime
    /// (identity only: Harness does not hash multi-GB files on a listing).
    pub digest: String,
}

type Cache = Mutex<HashMap<PathBuf, (SystemTime, Arc<FileInfo>)>>;

fn cache() -> &'static Cache {
    static C: OnceLock<Cache> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Shards of a split GGUF (`-00001-of-00003.gguf`), else the file itself.
fn shards(path: &Path) -> Vec<PathBuf> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let Some(pos) = name.find("-00001-of-") else {
        return vec![path.to_path_buf()];
    };
    let rest = &name[pos + "-00001-of-".len()..];
    let Some(count) = rest.get(..5).and_then(|c| c.parse::<u32>().ok()) else {
        return vec![path.to_path_buf()];
    };
    (1..=count)
        .map(|i| {
            path.with_file_name(format!(
                "{}-{:05}-of-{}",
                &name[..pos],
                i,
                &name[pos + "-00001-of-".len()..]
            ))
        })
        .collect()
}

/// Facts of `path` (blocking; cached until the file changes).
pub fn file_info(path: &Path) -> Option<Arc<FileInfo>> {
    let md = std::fs::metadata(path).ok()?;
    let modified = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    if let Some((m, info)) = cache().lock().ok()?.get(path) {
        if *m == modified {
            return Some(info.clone());
        }
    }
    let size = shards(path)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum::<u64>()
        .max(md.len());
    let meta = harness_core::gguf::read_gguf_metadata(path).ok();
    let kind = detect_kind(meta.as_ref());
    let digest = harness_core::downloader::recorded_hashes()
        .get(&path.to_string_lossy().to_string())
        .cloned()
        .unwrap_or_else(|| {
            let mut h = Sha256::new();
            h.update(path.to_string_lossy().as_bytes());
            h.update(size.to_le_bytes());
            h.update(
                modified
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_le_bytes(),
            );
            format!("{:x}", h.finalize())
        });
    let info = Arc::new(FileInfo {
        meta,
        kind,
        size,
        modified,
        digest,
    });
    if let Ok(mut c) = cache().lock() {
        c.insert(path.to_path_buf(), (modified, info.clone()));
    }
    Some(info)
}

/// Ollama `details`.
pub fn details(info: &FileInfo) -> Value {
    let m = info.meta.as_ref();
    let family = m.and_then(|m| m.architecture.clone()).unwrap_or_default();
    json!({
        "parent_model": "",
        "format": "gguf",
        "family": family,
        "families": if family.is_empty() { Value::Null } else { json!([family]) },
        "parameter_size": m.and_then(|m| m.param_count).map(convert::parameter_size)
            .or_else(|| m.and_then(|m| m.size_label.clone()))
            .unwrap_or_default(),
        "quantization_level": m.map(|m| m.quantization.clone()).unwrap_or_default(),
    })
}

/// Ollama capabilities: `completion`, `tools`, `thinking`, `vision`, `embedding`.
pub fn capabilities(def: &ModelDefinition, info: &FileInfo) -> Vec<&'static str> {
    match info.kind {
        ModelKind::Embedding => vec!["embedding"],
        // Ollama has no reranker capability; `/v1/rerank` serves these.
        ModelKind::Reranker => Vec::new(),
        _ => {
            let mut caps = vec!["completion"];
            let m = info.meta.as_ref();
            if m.is_some_and(|m| m.template_supports_tools) {
                caps.push("tools");
            }
            if m.is_some_and(|m| m.template_supports_reasoning) {
                caps.push("thinking");
            }
            if def.mmproj.is_some() {
                caps.push("vision");
            }
            caps
        }
    }
}

/// `/api/tags` entry of a definition.
pub fn tag_entry(def: &ModelDefinition, info: &FileInfo) -> Value {
    let name = convert::ollama_name(&def.alias);
    json!({
        "name": name,
        "model": name,
        "modified_at": convert::rfc3339(info.modified),
        "size": info.size,
        "digest": info.digest,
        "details": details(info),
        "capabilities": capabilities(def, info),
    })
}

// --- full key/value read (`/api/show`) ---

const MAX_INLINE_ARRAY: u64 = 64;

struct Reader {
    r: BufReader<File>,
}

impl Reader {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], String> {
        let mut b = [0u8; N];
        self.r.read_exact(&mut b).map_err(|e| e.to_string())?;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes::<4>()?))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes::<8>()?))
    }
    fn string(&mut self) -> Result<String, String> {
        let len = self.u64()?;
        if len > 64 * 1024 * 1024 {
            return Err("GGUF string too long".into());
        }
        let mut b = vec![0u8; len as usize];
        self.r.read_exact(&mut b).map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&b).into_owned())
    }
    fn scalar(&mut self, t: u32) -> Result<Value, String> {
        Ok(match t {
            0 => json!(self.bytes::<1>()?[0]),
            1 => json!(self.bytes::<1>()?[0] as i8),
            2 => json!(u16::from_le_bytes(self.bytes::<2>()?)),
            3 => json!(i16::from_le_bytes(self.bytes::<2>()?)),
            4 => json!(self.u32()?),
            5 => json!(i32::from_le_bytes(self.bytes::<4>()?)),
            6 => json!(f32::from_le_bytes(self.bytes::<4>()?)),
            7 => json!(self.bytes::<1>()?[0] != 0),
            8 => json!(self.string()?),
            10 => json!(self.u64()?),
            11 => json!(i64::from_le_bytes(self.bytes::<8>()?)),
            12 => json!(f64::from_le_bytes(self.bytes::<8>()?)),
            _ => return Err(format!("unknown GGUF value type {}", t)),
        })
    }
    fn value(&mut self, t: u32, verbose: bool) -> Result<Value, String> {
        if t != 9 {
            return self.scalar(t);
        }
        let et = self.u32()?;
        let n = self.u64()?;
        let keep = verbose || n <= MAX_INLINE_ARRAY;
        let mut out = Vec::new();
        for _ in 0..n {
            let v = self.value(et, verbose)?;
            if keep {
                out.push(v);
            }
        }
        Ok(Value::Array(out))
    }
}

/// Every key/value of a GGUF header (`model_info`) and the chat template.
pub fn key_values(
    path: &Path,
    verbose: bool,
) -> Result<(Map<String, Value>, Option<String>), String> {
    let f = File::open(path).map_err(|e| e.to_string())?;
    let mut r = Reader {
        r: BufReader::with_capacity(1 << 20, f),
    };
    if &r.bytes::<4>()? != b"GGUF" {
        return Err("not a GGUF file".into());
    }
    let version = r.u32()?;
    if version < 2 {
        return Err(format!("GGUF v{} is not supported", version));
    }
    let _tensors = r.u64()?;
    let count = r.u64()?;
    let mut out = Map::new();
    let mut template = None;
    for _ in 0..count {
        let key = r.string()?;
        let t = r.u32()?;
        let v = r.value(t, verbose)?;
        if key == "tokenizer.chat_template" {
            template = v.as_str().map(str::to_string);
            continue;
        }
        if key.starts_with("tokenizer.chat_template.") {
            continue;
        }
        out.insert(key, v);
    }
    Ok((out, template))
}

/// `/api/show` body of a definition (blocking: reads the GGUF header).
pub fn show(def: &ModelDefinition, path: &Path, verbose: bool) -> Result<Value, String> {
    let info = file_info(path).ok_or_else(|| format!("{} is not readable", path.display()))?;
    let (mut model_info, template) = key_values(path, verbose)?;
    if !model_info.contains_key("general.parameter_count") {
        if let Some(n) = info.meta.as_ref().and_then(|m| m.param_count) {
            model_info.insert("general.parameter_count".into(), json!(n));
        }
    }
    let mut modelfile = format!(
        "# Modelfile generated by Harness Studio (Ollama-compatible API)\n# Model definition: {} ({})\nFROM {}\n",
        def.alias,
        def.id,
        path.display()
    );
    for a in &def.adapters {
        modelfile.push_str(&format!("ADAPTER {}\n", a.path.display()));
    }
    let ctx = def
        .load
        .get("context_size")
        .and_then(|v| serde_json::to_value(v).ok())
        .and_then(|v| v.as_i64())
        .filter(|n| *n > 0);
    let parameters = ctx.map(|n| format!("num_ctx {}", n)).unwrap_or_default();
    if !parameters.is_empty() {
        modelfile.push_str(&format!("PARAMETER {}\n", parameters));
    }
    let mut out = json!({
        "modelfile": modelfile,
        "parameters": parameters,
        "template": template.unwrap_or_default(),
        "details": details(&info),
        "model_info": model_info,
        "capabilities": capabilities(def, &info),
        "modified_at": convert::rfc3339(info.modified),
    });
    if let Some(mm) = def.mmproj.as_deref().filter(|p| p.is_file()) {
        if let Ok((projector, _)) = key_values(mm, false) {
            out["projector_info"] = Value::Object(projector);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_shards() {
        let s = shards(Path::new("/m/Llama-70B-Q4_K_M-00001-of-00003.gguf"));
        assert_eq!(s.len(), 3);
        assert!(s[2].ends_with("Llama-70B-Q4_K_M-00003-of-00003.gguf"));
        assert_eq!(shards(Path::new("/m/a.gguf")).len(), 1);
    }
}
