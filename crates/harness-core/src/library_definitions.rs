//! Default definitions for library files that were found by a scan instead of downloaded.
//!
//! The gateway (`/v1/models`) and the Playground pickers list model definitions, not files. A
//! Hub download makes its definition when it finishes; a file that was already in a library
//! folder never went through that. This makes the same plain definition (alias from the file
//! name, no settings of its own, `auto_created` off like every definition saved by the app)
//! for the kinds that are served through a definition and cannot get one from "Load and Chat":
//! image models (with the companion files they were found with), whisper speech models,
//! embedding models, rerankers and ONNX task models (`pipeline`). Chat models get theirs when they are loaded; Kokoro / Piper
//! text-to-speech and sherpa-onnx recognition models run per request and need none.

use std::path::PathBuf;

use crate::domain::store::Domain;
use crate::domain::{unique_alias, LocalModel, ModelDefinition, ModelKind, ModelRef};

/// The kinds that get a default definition.
pub const KINDS: [ModelKind; 6] = [
    ModelKind::Image,
    ModelKind::Asr,
    ModelKind::Embedding,
    ModelKind::Reranker,
    ModelKind::Pipeline,
    ModelKind::Tts,
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
    remove_stale_diffusers_definitions(domain, libraries)?;
    let mut created = Vec::new();
    for model in models
        .iter()
        .filter(|m| m.is_ready && KINDS.contains(&m.kind) && kinds.contains(&m.kind))
        // Also guard callers with a stale scan, including download-completion syncs.
        .filter(|m| !crate::models::is_diffusers_component(&m.path))
        // sherpa-onnx recognition models are ASR too, but run per request without a definition.
        .filter(|m| {
            m.kind != ModelKind::Tts
                || crate::engine::audio_cpp::is_tts(&m.path)
                || crate::engine::crispasr_tts::is_model(&m.path)
                || crate::tts::llama::companion_projector(&m.path).is_some()
        })
        .filter(|m| !(m.kind == ModelKind::Asr && crate::asr::is_sherpa_file(&m.path)))
    {
        let defs = domain.definitions()?;
        if let Some(mut def) = defs
            .iter()
            .find(|d| d.base.local_path() == Some(model.path.as_path()))
            .cloned()
        {
            if crate::asr::is_onnx_ctc_file(&model.path)
                && def.load.get("task") != Some(&crate::params::ParamValue::Text("asr_ctc".into()))
            {
                def.load.insert(
                    "task".into(),
                    crate::params::ParamValue::Text("asr_ctc".into()),
                );
                domain.save_definition(def)?;
                continue;
            }
            // A repo's own vision projector that arrived after the definition was made
            // replaces the curated one.
            let current = def.load.get("llm_vision").and_then(|v| match v {
                crate::params::ParamValue::Text(t) => Some(t.as_str()),
                _ => None,
            });
            let curated = crate::image_bundles::bundle_for(&model.filename)
                .and_then(|b| b.components.iter().find(|c| c.role == "llm_vision"));
            let may_update = current.is_none_or(str::is_empty)
                || curated.is_some_and(|c| {
                    current.is_some_and(|p| {
                        libraries
                            .iter()
                            .any(|l| std::path::Path::new(p) == l.join(c.repo).join(c.file))
                            || model.path.parent().is_some_and(|d| {
                                std::path::Path::new(p)
                                    == d.join(c.file.rsplit('/').next().unwrap_or(c.file))
                            })
                    })
                });
            if model.kind == ModelKind::Image && may_update {
                if let Some(own) = crate::image_bundles::own_projector(&model.path, libraries) {
                    let own = crate::params::ParamValue::Text(own.to_string_lossy().to_string());
                    if def.load.get("llm_vision") != Some(&own) {
                        def.load.insert("llm_vision".into(), own);
                        domain.save_definition(def)?;
                    }
                }
            }
            continue;
        }
        let load = if crate::engine::transcribe_cpp::is_model(&model.path) {
            [(
                "task".into(),
                crate::params::ParamValue::Text(crate::engine::transcribe_cpp::TASK.into()),
            )]
            .into()
        } else if crate::engine::crispasr::is_model(&model.path) {
            [(
                "task".into(),
                crate::params::ParamValue::Text("crispasr".into()),
            )]
            .into()
        } else if crate::asr::is_onnx_ctc_file(&model.path) {
            [(
                "task".into(),
                crate::params::ParamValue::Text("asr_ctc".into()),
            )]
            .into()
        } else if model.kind == ModelKind::Image {
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
        // Qwen3-TTS speaks through llama-tts with its projector next to it.
        if model.kind == ModelKind::Tts && crate::tts::llama::is_tts_file(&model.path) {
            def.mmproj = crate::tts::llama::companion_projector(&model.path);
        }
        created.push(domain.save_definition(def)?.alias);
    }
    Ok(created)
}

/// Older scans saved components without provenance (`auto_created` is false even for
/// library definitions). Only remove an exact default-shaped record; any customization
/// or profile reference makes it user-owned. Do not depend on the now-filtered scan.
fn remove_stale_diffusers_definitions(
    domain: &Domain,
    libraries: &[PathBuf],
) -> Result<(), String> {
    let defs = domain.definitions()?;
    let profiles = domain.profiles()?;
    for def in &defs {
        let Some(path) = def.base.local_path() else {
            continue;
        };
        if !path.is_file()
            || !crate::models::is_diffusers_component(path)
            || def.created_at != def.updated_at
            || profiles
                .iter()
                .any(|p| p.definition_id.as_deref() == Some(&def.id))
        {
            continue;
        }
        let Some(stem) = path.file_stem() else {
            continue;
        };
        let alias = unique_alias(
            &stem.to_string_lossy(),
            defs.iter()
                .filter(|other| other.id != def.id)
                .map(|other| other.alias.as_str()),
        );
        let mut expected = ModelDefinition::new(alias, def.base.clone());
        if expected.load.is_empty() {
            let Some(load) = crate::image_bundles::definition_load(path, libraries, true) else {
                continue;
            };
            expected.load = load;
        }
        expected.id.clone_from(&def.id);
        expected.created_at = def.created_at;
        expected.updated_at = def.updated_at;
        if *def == expected {
            domain.delete_definition(&def.id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_diffusers_defaults_are_removed_but_user_and_folder_definitions_survive() {
        let cfg = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        let domain = Domain::at(cfg.path());
        let pipeline = lib.path().join("pipeline");
        std::fs::create_dir_all(pipeline.join("unet")).unwrap();
        let component = file(
            &pipeline.join("unet"),
            "diffusion_pytorch_model.fp16.safetensors",
            ModelKind::Image,
            true,
        );
        let libraries = [lib.path().to_path_buf()];
        // Reproduce the old scanner before the pipeline marker is present. Library
        // definitions deliberately have auto_created=false, so that flag cannot help.
        ensure_in(&domain, &libraries, &[component], &KINDS).unwrap();
        let stale = domain.definitions().unwrap().remove(0);
        assert!(!stale.auto_created);
        assert_eq!(stale.alias, "diffusion_pytorch_model.fp16");
        let user = domain
            .save_definition(ModelDefinition::new("my-unet", stale.base.clone()))
            .unwrap();
        // Every configurable field participates in the comparison, even with the
        // generated alias. Timestamps can be identical for edits within one second.
        let customized = file(&pipeline, "unet/edited.ckpt", ModelKind::Image, true);
        let mut edited = ModelDefinition::new(
            "edited",
            ModelRef::Local {
                path: customized.path,
            },
        );
        edited.description = "My settings".into();
        let edited = domain.save_definition(edited).unwrap();
        std::fs::write(pipeline.join("model_index.json"), "{}").unwrap();
        let folder = domain
            .save_definition(ModelDefinition::new(
                "pipeline",
                ModelRef::Local { path: pipeline },
            ))
            .unwrap();

        // Cleanup also runs with an empty scan and a non-image kind selection.
        assert!(ensure_in(&domain, &libraries, &[], &[ModelKind::Asr])
            .unwrap()
            .is_empty());
        assert_eq!(domain.definitions().unwrap(), vec![user, edited, folder]);
        assert!(ensure_in(&domain, &libraries, &[], &KINDS)
            .unwrap()
            .is_empty());
    }

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
            task: None,
        }
    }

    #[test]
    fn diffusers_components_from_a_stale_scan_get_no_definitions() {
        let cfg = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        let domain = Domain::at(cfg.path());
        let pipeline = lib.path().join("pipeline");
        std::fs::create_dir_all(pipeline.join("unet/nested")).unwrap();
        std::fs::create_dir_all(pipeline.join("transformer")).unwrap();
        std::fs::create_dir_all(pipeline.join("vae")).unwrap();
        std::fs::create_dir_all(pipeline.join("text_encoder")).unwrap();
        let mut stale = Vec::new();
        for name in [
            "weights.ckpt",
            "unet/diffusion_pytorch_model.fp16.safetensors",
            "unet/nested/weights.ckpt",
            "transformer/diffusion_pytorch_model.safetensors",
            "vae/diffusion_pytorch_model.safetensors",
            "text_encoder/model.safetensors",
        ] {
            stale.push(file(&pipeline, name, ModelKind::Image, true));
        }
        std::fs::write(
            pipeline.join("model_index.json"),
            r#"{"_class_name":"StableDiffusionPipeline"}"#,
        )
        .unwrap();
        let standalone = file(lib.path(), "standalone.ckpt", ModelKind::Image, true);
        let libraries = [lib.path().to_path_buf()];
        let mut models = crate::models::ModelManager.scan_directories(&libraries);
        models.extend(stale);
        // Both Library and Hub completion sync through this entry point. A stale scan must
        // not register component definitions, even when the files still exist.
        let made = ensure_in(&domain, &libraries, &models, &KINDS).unwrap();
        assert_eq!(made.len(), 2);
        let defs = domain.definitions().unwrap();
        assert_eq!(defs.len(), 2);
        assert!(defs
            .iter()
            .any(|d| d.base.local_path() == Some(pipeline.as_path())));
        assert!(defs
            .iter()
            .any(|d| d.base.local_path() == Some(standalone.path.as_path())));
        assert!(ensure_in(&domain, &libraries, &models, &KINDS)
            .unwrap()
            .is_empty());
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
    fn ctc_gets_an_onnx_definition_and_speech_capability() {
        let cfg = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        std::fs::create_dir(lib.path().join("onnx")).unwrap();
        std::fs::write(
            lib.path().join("config.json"),
            r#"{"architectures":["Wav2Vec2ForCTC"],"vocab_size":3,"pad_token_id":0}"#,
        )
        .unwrap();
        std::fs::write(
            lib.path().join("preprocessor_config.json"),
            r#"{"sampling_rate":16000,"feature_size":1}"#,
        )
        .unwrap();
        std::fs::write(lib.path().join("vocab.json"), r#"{"<pad>":0,"a":1,"|":2}"#).unwrap();
        let model = file(lib.path(), "onnx/model.onnx", ModelKind::Asr, true);
        assert!(crate::asr::is_onnx_ctc_file(&model.path));
        assert!(!crate::asr::is_sherpa_file(&model.path));
        let domain = Domain::at(cfg.path());
        ensure_in(
            &domain,
            &[],
            std::slice::from_ref(&model),
            &[ModelKind::Asr],
        )
        .unwrap();
        let mut def = domain.definitions().unwrap().remove(0);
        def.load.clear();
        let def = domain.save_definition(def).unwrap();
        assert_eq!(
            def.load["task"],
            crate::params::ParamValue::Text("asr_ctc".into())
        );
        let caps = crate::library::ModelCaps {
            asr: true,
            ..Default::default()
        };
        let record = crate::model_registry::local_record(&model, &caps);
        assert_eq!(record.runtime.as_deref(), Some(crate::engine::ONNX_FAMILY));
        assert_eq!(record.format, crate::hub_compat::ModelFormat::Onnx);
    }

    #[test]
    fn a_repos_own_projector_replaces_the_curated_one_when_it_arrives() {
        let cfg = tempfile::tempdir().unwrap();
        let lib = tempfile::tempdir().unwrap();
        let domain = Domain::at(cfg.path());
        let name = "qwen-image-edit-2511-uncensored-Q4_K_M.gguf";
        // The curated companions are in the library already.
        let plan = crate::image_bundles::plan_for(name, &[], None).unwrap();
        for c in &plan.components {
            let p = lib.path().join(&c.repo).join(&c.file);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        let split = lib.path().join("o/uncensored/split");
        std::fs::create_dir_all(split.join("diffusion_models")).unwrap();
        std::fs::create_dir_all(split.join("text_encoders")).unwrap();
        let main = file(
            &split.join("diffusion_models"),
            name,
            ModelKind::Image,
            true,
        );
        let libs = [lib.path().to_path_buf()];
        let made = ensure_in(&domain, &libs, std::slice::from_ref(&main), &KINDS).unwrap();
        assert_eq!(made.len(), 1);
        let projector = |d: &Domain| match d.definitions().unwrap()[0].load.get("llm_vision") {
            Some(crate::params::ParamValue::Text(t)) => t.replace('\\', "/"),
            other => panic!("{other:?}"),
        };
        assert!(projector(&domain).ends_with("Qwen2.5-VL-7B-Instruct.mmproj-Q8_0.gguf"));
        // The repo's own projector is downloaded afterwards.
        let own = split.join("text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf");
        std::fs::write(&own, b"x").unwrap();
        assert!(
            ensure_in(&domain, &libs, std::slice::from_ref(&main), &KINDS)
                .unwrap()
                .is_empty()
        );
        assert!(
            projector(&domain).ends_with("text_encoders/Qwen2.5-VL-7B-Instruct-mmproj-f16.gguf")
        );
        // A later scan preserves a user-selected projector, even when the repo has its own.
        let mut def = domain.definitions().unwrap().remove(0);
        def.load.insert(
            "llm_vision".into(),
            crate::params::ParamValue::Text("custom/vision.gguf".into()),
        );
        domain.save_definition(def).unwrap();
        ensure_in(&domain, &libs, std::slice::from_ref(&main), &KINDS).unwrap();
        assert_eq!(projector(&domain), "custom/vision.gguf");
        // Empty values can still be filled automatically.
        let mut def = domain.definitions().unwrap().remove(0);
        def.load.insert(
            "llm_vision".into(),
            crate::params::ParamValue::Text(String::new()),
        );
        domain.save_definition(def).unwrap();
        ensure_in(&domain, &libs, std::slice::from_ref(&main), &KINDS).unwrap();
        assert_eq!(projector(&domain), own.to_string_lossy().replace('\\', "/"));
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
