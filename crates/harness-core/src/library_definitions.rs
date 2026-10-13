//! Default definitions for library files that were found by a scan instead of downloaded.
//!
//! The gateway (`/v1/models`) and the Playground pickers list model definitions, not files. A
//! Hub download makes its definition when it finishes; a file that was already in a library
//! folder never went through that. This makes the same plain definition (alias from the file
//! name, no settings of its own, `auto_created` off like every definition saved by the app)
//! for the kinds that are served through a definition and cannot get one from "Load and Chat":
//! image models (with the companion files they were found with), whisper speech models,
//! embedding models and rerankers. Chat models get theirs when they are loaded; Kokoro / Piper
//! text-to-speech and sherpa-onnx recognition models run per request and need none.

use std::path::PathBuf;

use crate::domain::store::Domain;
use crate::domain::{unique_alias, LocalModel, ModelDefinition, ModelKind, ModelRef};

/// The kinds that get a default definition.
pub const KINDS: [ModelKind; 4] = [
    ModelKind::Image,
    ModelKind::Asr,
    ModelKind::Embedding,
    ModelKind::Reranker,
];

/// [`ensure_in`] for the app's own domain files and library folders.
pub fn ensure(models: &[LocalModel], kinds: &[ModelKind]) -> Result<Vec<String>, String> {
    let libraries = crate::config::AppSettings::load().all_models_dirs();
    ensure_in(&Domain::global(), &libraries, models, kinds)
}

/// Makes a default definition for every ready file of `kinds` (see [`KINDS`]) that has no
/// definition yet and returns the aliases it made. Idempotent. An image model whose companion
/// files are not all there yet is skipped and picked up by a later call.
pub fn ensure_in(
    domain: &Domain,
    libraries: &[PathBuf],
    models: &[LocalModel],
    kinds: &[ModelKind],
) -> Result<Vec<String>, String> {
    let mut created = Vec::new();
    for model in models
        .iter()
        .filter(|m| m.is_ready && KINDS.contains(&m.kind) && kinds.contains(&m.kind))
        // sherpa-onnx recognition models are ASR too, but run per request without a definition.
        .filter(|m| !(m.kind == ModelKind::Asr && crate::asr::is_sherpa_file(&m.path)))
    {
        let defs = domain.definitions()?;
        if defs
            .iter()
            .any(|d| d.base.local_path() == Some(model.path.as_path()))
        {
            continue;
        }
        let load = if model.kind == ModelKind::Image {
            let Some(load) = crate::image_bundles::definition_load(&model.path, libraries, true)
            else {
                continue;
            };
            load
        } else {
            Default::default()
        };
        let alias = unique_alias(&model.name, defs.iter().map(|d| d.alias.as_str()));
        let mut def = ModelDefinition::new(
            alias,
            ModelRef::Local {
                path: model.path.clone(),
            },
        );
        def.load = load;
        created.push(domain.save_definition(def)?.alias);
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &std::path::Path, name: &str, kind: ModelKind, ready: bool) -> LocalModel {
        let path = dir.join(name);
        std::fs::write(&path, b"x").unwrap();
        LocalModel {
            id: name.to_string(),
            name: name.rsplit_once('.').map_or(name, |(s, _)| s).to_string(),
            filename: name.to_string(),
            path,
            size_bytes: 1,
            architecture: None,
            context_length: None,
            quantization: String::new(),
            is_split: false,
            total_shards: None,
            present_shards: None,
            is_ready: ready,
            shard_paths: Vec::new(),
            root_dir: dir.display().to_string(),
            gguf: None,
            source: None,
            mmproj_path: None,
            kind,
            origin: Default::default(),
        }
    }

    #[test]
    fn scanned_files_of_the_asked_kinds_get_a_plain_definition_once() {
        let cfg = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        let domain = Domain::at(cfg.path());
        let models = vec![
            file(lib.path(), "sd_turbo-f16-q8_0.gguf", ModelKind::Image, true),
            file(lib.path(), "ggml-base.bin", ModelKind::Asr, true),
            file(lib.path(), "bge-m3.gguf", ModelKind::Embedding, true),
            file(lib.path(), "bge-reranker.gguf", ModelKind::Reranker, true),
            file(lib.path(), "chat.gguf", ModelKind::Llm, true),
            file(lib.path(), "half.bin", ModelKind::Asr, false),
            file(
                lib.path(),
                "parakeet-encoder.int8.onnx",
                ModelKind::Asr,
                true,
            ),
        ];
        let made = ensure_in(
            &domain,
            &[lib.path().to_path_buf()],
            &models,
            &[ModelKind::Asr, ModelKind::Embedding, ModelKind::Reranker],
        )
        .unwrap();
        assert_eq!(made, vec!["ggml-base", "bge-m3", "bge-reranker"]);
        let defs = domain.definitions().unwrap();
        assert_eq!(defs.len(), 3);
        assert!(defs.iter().all(|d| !d.auto_created && d.load.is_empty()));

        // Idempotent, and another kind is added on its own call.
        let again = ensure_in(&domain, &[lib.path().to_path_buf()], &models, &KINDS).unwrap();
        assert_eq!(again, vec!["sd_turbo-f16-q8_0"]);
        assert!(
            ensure_in(&domain, &[lib.path().to_path_buf()], &models, &KINDS)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn an_alias_that_is_taken_gets_a_suffix() {
        let cfg = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        let domain = Domain::at(cfg.path());
        let a = file(lib.path(), "ggml-base.bin", ModelKind::Asr, true);
        let sub = lib.path().join("other");
        std::fs::create_dir_all(&sub).unwrap();
        let b = file(&sub, "ggml-base.bin", ModelKind::Asr, true);
        let made = ensure_in(&domain, &[], &[a, b], &[ModelKind::Asr]).unwrap();
        assert_eq!(made, vec!["ggml-base", "ggml-base-2"]);
    }
}
