use super::*;
use crate::hub_compat::{Compatibility, ModelFormat};
use crate::hub_tasks::NotRunnable;

fn fixture(name: &str) -> serde_json::Value {
    serde_json::from_str(match name {
        "vieneu" => include_str!("../tests/data/hub/vieneu.json"),
        "nemotron" => include_str!("../tests/data/hub/nemotron.json"),
        "lightning" => include_str!("../tests/data/hub/lightning.json"),
        "whisper" => include_str!("../tests/data/hub/whisper.json"),
        _ => unreachable!(),
    })
    .unwrap()
}

#[test]
fn vieneu_metadata_refuses_listing_and_both_variants_without_downloads() {
    let f = fixture("vieneu");
    assert_eq!(f["header"]["audiocpp.model_spec.family"], "vieneu_v3_turbo");
    assert_eq!(
        serde_json::to_value(NotRunnable::NeedsSeaG2p).unwrap(),
        "needs_sea_g2p"
    );
    for item in [&f["listing"], &f["detail"]] {
        let listed = parse_search_item(item).unwrap();
        assert_eq!(listed.not_runnable, Some(NotRunnable::NeedsSeaG2p));
        assert_eq!(
            listed.compatibility,
            Compatibility::Unsupported {
                format: ModelFormat::Gguf
            }
        );
        assert_eq!(
            card_not_runnable(item, Some(NotRunnable::NoGguf)),
            Some(NotRunnable::NeedsSeaG2p)
        );
        let variants = audio_cpp_variants(
            &listed.id,
            f["tree"].as_array().unwrap(),
            crate::hub_compat::audio_cpp_not_runnable(item),
        )
        .unwrap();
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().any(|v| v.name.ends_with("bf16")));
        assert!(variants.iter().any(|v| v.name.ends_with("q8_0")));
        for variant in variants {
            assert_eq!(variant.not_runnable, listed.not_runnable);
            assert!(variant.shards.is_empty());
        }
    }
    // Neither a repo name, config model_type nor folder layout is required.
    for family_key in ["audiocpp.model_spec.family", "family"] {
        let item = serde_json::json!({
            "id": "other/export", "gguf": {family_key: "vieneu_v3_turbo"},
            "siblings": [{"rfilename":"model.gguf"}]
        });
        assert_eq!(
            parse_search_item(&item).unwrap().not_runnable,
            Some(NotRunnable::NeedsSeaG2p)
        );
    }
    // Config alone must not classify a source-only export as an audio.cpp GGUF.
    let mut source_only = f["listing"].clone();
    source_only["siblings"] = serde_json::json!([{"rfilename":"model.safetensors"}]);
    assert_eq!(
        crate::hub_compat::audio_cpp_not_runnable(&source_only),
        None
    );
    let mut unrelated = f["listing"].clone();
    unrelated["gguf"] =
        serde_json::json!({"architecture":"audiocpp", "audiocpp.model_spec.family":"pocket_tts"});
    assert_eq!(parse_search_item(&unrelated).unwrap().not_runnable, None);
    assert!(
        audio_cpp_variants("other/repo", f["tree"].as_array().unwrap(), None)
            .unwrap()
            .iter()
            .all(|v| v.not_runnable == Some(NotRunnable::AudioCppUnsupported))
    );
}

#[test]
fn nemotron_native_folder_wins_over_unsupported_gguf() {
    let f = fixture("nemotron");
    assert_eq!(f["processor"]["processor_class"], "Nemotron3_5AsrProcessor");
    assert_eq!(f["processor"]["feature_extractor"]["sampling_rate"], 16000);
    assert_eq!(f["config"]["architectures"][0], "Nemotron3_5AsrForRNNT");
    for item in [&f["listing"], &f["detail"]] {
        assert_eq!(item["library_name"], "nemo");
        let listed = parse_search_item(item).unwrap();
        assert_eq!(listed.format, ModelFormat::Safetensors);
        assert_eq!(listed.not_runnable, Some(NotRunnable::NeedsLibrosa));
        assert_eq!(
            card_not_runnable(item, Some(NotRunnable::NoGguf)),
            Some(NotRunnable::NeedsLibrosa)
        );
        assert_eq!(
            listed.compatibility,
            Compatibility::Unsupported {
                format: ModelFormat::Safetensors
            }
        );
        assert_eq!(
            crate::hub_compat::native_speech_format(item, &sibling_files(item)),
            Some(ModelFormat::Safetensors)
        );
        let variants = safetensors_variants(&listed.id, f["tree"].as_array().unwrap()).unwrap();
        assert_eq!(variants.len(), 1);
        let files: Vec<_> = variants[0]
            .shards
            .iter()
            .map(|s| s.filename.as_str())
            .collect();
        for file in [
            "model.safetensors",
            "config.json",
            "processor_config.json",
            "tokenizer.json",
            "tokenizer_config.json",
        ] {
            assert!(files.contains(&file), "{file}");
        }
        assert!(!files
            .iter()
            .any(|f| f.ends_with(".gguf") || f.ends_with(".nemo")));
        let mut incomplete = sibling_files(item);
        incomplete.retain(|f| f != "model.safetensors");
        assert_eq!(
            crate::hub_compat::native_speech_format(item, &incomplete),
            None
        );
    }
    for model_type in [
        "nemotron3_5_asr",
        "nemotron_asr_streaming",
        "parakeet_rnnt",
        "parakeet_tdt",
    ] {
        assert!(crate::engine::python_hf::serves_model_type(
            Some("automatic-speech-recognition"),
            Some(model_type)
        ));
    }
    assert!(!crate::engine::crispasr::supports_repo(
        "nvidia/nemotron-3.5-asr-streaming-0.6b",
        Some("automatic-speech-recognition")
    ));
}

#[test]
fn missing_processor_dependencies_refuse_python_folders_without_hiding_native_format() {
    let f = fixture("nemotron");
    for model_type in [
        "nemotron3_5_asr",
        "nemotron_asr_streaming",
        "parakeet_ctc",
        "parakeet_rnnt",
        "parakeet_tdt",
    ] {
        assert_eq!(
            crate::engine::python_hf::missing_dependency(Some(model_type)),
            Some("librosa")
        );
        for with_gguf in [true, false] {
            let mut item = f["listing"].clone();
            item["config"]["model_type"] = model_type.into();
            if !with_gguf {
                item["siblings"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|s| !s["rfilename"].as_str().unwrap().ends_with(".gguf"));
            }
            let listed = parse_search_item(&item).unwrap();
            assert_eq!(listed.not_runnable, Some(NotRunnable::NeedsLibrosa));
            assert_eq!(
                listed.compatibility,
                Compatibility::Unsupported {
                    format: ModelFormat::Safetensors
                }
            );
        }
    }
    // An ordinary supported processor remains runnable through the same native route.
    let mut item = f["listing"].clone();
    item["config"]["model_type"] = "whisper".into();
    let listed = parse_search_item(&item).unwrap();
    assert_eq!(listed.not_runnable, None);
    assert_eq!(
        listed.compatibility,
        Compatibility::Runnable {
            runtime: crate::engine::PYTHON_HF.into()
        }
    );
    assert_eq!(crate::engine::python_hf::missing_dependency(None), None);
    for lock in [
        include_str!("../python/hf-cpu.lock"),
        include_str!("../python/hf-cuda-linux.lock"),
        include_str!("../python/hf-cuda-windows.lock"),
    ] {
        assert!(!lock.lines().any(|line| line.starts_with("librosa==")));
    }
    assert_eq!(
        serde_json::to_value(NotRunnable::NeedsLibrosa).unwrap(),
        "needs_librosa"
    );
}

#[test]
fn lightning_full_checkpoints_and_adapters_are_classified_separately() {
    let f = fixture("lightning");
    let item = &f["listing"];
    let listed = parse_search_item(item).unwrap();
    assert_eq!(listed.not_runnable, None);
    assert_eq!(
        listed.compatibility,
        Compatibility::Runnable {
            runtime: crate::hub_compat::SDCPP_FAMILY.into()
        }
    );
    let variants =
        DownloadManager::file_variants(&listed.id, f["tree"].as_array().unwrap(), true).unwrap();
    assert_eq!(variants.len(), 8);
    for v in &variants {
        let file = &v.shards[0].filename;
        if crate::image_bundles::is_standalone_checkpoint(file) {
            assert!(v.total_size_bytes > 20_000_000_000);
            assert_eq!(v.not_runnable, None);
            let plan = crate::image_bundles::plan_for(file, &[], None).unwrap();
            assert_eq!(plan.bundle_id, "qwen-image-edit-2511");
            assert_eq!(
                plan.components
                    .iter()
                    .map(|c| c.role.as_str())
                    .collect::<Vec<_>>(),
                ["vae", "llm", "llm_vision"]
            );
            assert_eq!(
                plan.load().get("model_args"),
                Some(&crate::params::ParamValue::Text(
                    "qwen_image_zero_cond_t=true".into()
                ))
            );
        } else {
            assert_eq!(v.quantization, "component");
            assert_eq!(v.not_runnable, Some(NotRunnable::Adapter));
            assert!(crate::image_bundles::bundle_for(file).is_none());
        }
    }
    assert_eq!(
        variants.iter().filter(|v| v.not_runnable.is_none()).count(),
        4
    );
    let mut adapters = item.clone();
    adapters["siblings"].as_array_mut().unwrap().retain(|s| {
        !crate::image_bundles::is_standalone_checkpoint(s["rfilename"].as_str().unwrap())
    });
    let listed = parse_search_item(&adapters).unwrap();
    assert_eq!(listed.not_runnable, Some(NotRunnable::Adapter));
    assert!(matches!(
        listed.compatibility,
        Compatibility::Unsupported { .. }
    ));
}

#[test]
fn exact_native_whisper_is_included_even_with_an_injected_result() {
    let native = parse_search_item(&fixture("whisper")["listing"]).unwrap();
    assert_eq!(
        native.compatibility,
        Compatibility::Runnable {
            runtime: crate::engine::PYTHON_HF.into()
        }
    );
    let injected = parse_search_item(&serde_json::json!({"id":"ggerganov/whisper.cpp", "siblings":[{"rfilename":"ggml-base.bin"}]})).unwrap();
    for task in [None, Some("automatic-speech-recognition".into())] {
        let mut options = HfSearchOptions {
            query: "openai/whisper-base".into(),
            pipeline_tag: task,
            ..Default::default()
        };
        let mut items = vec![injected.clone()];
        assert!(exact_repo_missing(&options, &items));
        items.push(native.clone());
        prioritize_exact_repo(&mut items, &options.query);
        assert_eq!(items[0].id, "openai/whisper-base");
        assert!(!exact_repo_missing(&options, &items));
        options.cursor = Some("next".into());
        assert!(!exact_repo_missing(&options, &[]));
        options.cursor = None;
        options.query = "whisper base".into();
        assert!(!exact_repo_missing(&options, &[]));
    }
}

#[test]
fn task_status_uses_image_and_speech_engines_without_llama_support() {
    let caps = crate::capabilities::testdata::caps("b11236");
    let mut features = crate::capabilities::features::feature_status(
        &caps,
        crate::capabilities::features::FeatureContext::default(),
    );
    for f in features.iter_mut().filter(|f| f.id == "audio_input") {
        f.level = crate::capabilities::features::CapabilityLevel::Unsupported;
        f.available = Some(false);
    }
    let rows =
        crate::hub_tasks::task_support(Some(&caps), &features, &[crate::engine::PYTHON_HF.into()]);
    let asr = rows
        .iter()
        .find(|r| r.task == "automatic-speech-recognition")
        .unwrap();
    assert_eq!(asr.support, crate::hub_tasks::Support::Yes);
    assert!(!asr.runtime_missing && !asr.verified);
    assert!(asr.flag.is_none());
    for family in [crate::hub_compat::SDCPP_FAMILY, crate::engine::PYTHON_HF] {
        let rows = crate::hub_tasks::task_support(None, &[], &[family.into()]);
        for task in ["text-to-image", "image-to-image"] {
            let row = rows.iter().find(|r| r.task == task).unwrap();
            assert_eq!(row.support, crate::hub_tasks::Support::Yes);
            assert_eq!(row.runtime.as_deref(), Some(family));
            assert!(!row.runtime_missing);
        }
    }
    for family in [
        crate::engine::PYTHON_HF,
        crate::engine::WHISPER_CPP,
        crate::engine::AUDIO_CPP,
    ] {
        let task = if family == crate::engine::AUDIO_CPP {
            "text-to-speech"
        } else {
            "automatic-speech-recognition"
        };
        let rows = crate::hub_tasks::task_support(None, &[], &[family.into()]);
        assert_eq!(
            rows.iter().find(|r| r.task == task).unwrap().support,
            crate::hub_tasks::Support::Yes
        );
    }
}
