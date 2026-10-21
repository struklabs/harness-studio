use super::arch::{scan, scan_dir};
use super::env;
use super::plan::{build_plan, estimate, merge_config, select_files, ConvertPlan};
use super::run::{
    mmproj_name, output_name, parse_line, parse_quantize_step, target_dir_in,
    unsupported_architecture_in, Line,
};
use super::*;
use serde_json::json;
use std::path::PathBuf;

/// Shaped like `conversion/*.py` of llama.cpp b11559 (decorators with several names, a
/// decorator spread over lines, stacked decorators, vision classes derived through another
/// class).
const SOURCE_A: &str = r#"
@ModelBase.register("LlamaForCausalLM", "MistralForCausalLM", "MixtralForCausalLM")
class LlamaModel(TextModel):
    model_arch = gguf.MODEL_ARCH.LLAMA


@ModelBase.register(
    "Qwen3ForCausalLM",
    "Qwen3Model",
)
class Qwen3Model(Qwen2Model):
    model_arch = gguf.MODEL_ARCH.QWEN3


@ModelBase.register(
    "LLaMAForCausalLM",
    "VLlama3ForCausalLM",
    "LlamaModel")
# [TAG_HF_EXAMPLE_GATED] meta-llama/Llama-3.2-1B-Instruct is gated
@ModelBase.example("unsloth/Llama-3.2-1B-Instruct", "mistralai/Mistral-7B-Instruct-v0.3")
class LlamaModel2(TextModel):
    model_arch = gguf.MODEL_ARCH.LLAMA


@ModelBase.register("LlavaForConditionalGeneration")
class LlavaVisionModel(MmprojModel):
    pass


@ModelBase.register("Idefics3ForConditionalGeneration", "SmolVLMForConditionalGeneration")
class SmolVLMModel(LlavaVisionModel):
    pass


@ModelBase.register("Idefics3ForConditionalGeneration", "SmolVLMForConditionalGeneration")
class SmolLMText(LlamaModel):
    pass

@ModelBase.register_hparams_loader(_is_decision_checkpoint)
def loader():
    pass

@ModelBase.register("ModernBertDecisionModel")
@some_other_decorator
class Decision(BertModel):
    pass
"#;

#[test]
fn architectures_are_read_from_the_register_decorators() {
    let a = scan(&[SOURCE_A]);
    for name in [
        "LlamaForCausalLM",
        "MistralForCausalLM",
        "VLlama3ForCausalLM",
        "LlamaModel",
        "Qwen3ForCausalLM",
        "Qwen3Model",
        "SmolVLMForConditionalGeneration",
        "ModernBertDecisionModel",
    ] {
        assert!(a.supports_text(name), "{name}");
    }
    // Vision classes are found through their base classes, across the whole source.
    assert!(a.supports_vision("LlavaForConditionalGeneration"));
    assert!(a.supports_vision("Idefics3ForConditionalGeneration"));
    // A name both classes register is in both lists; a text name is not a projector.
    assert!(a.supports_text("Idefics3ForConditionalGeneration"));
    assert!(!a.supports_vision("LlamaForCausalLM"));
    // Not registered, and a hparams loader is not a model class.
    assert!(!a.supports_text("LlavaForConditionalGeneration"));
    assert!(!a.supports_text("TotallyNewForCausalLM"));
}

#[test]
fn base_classes_in_other_files_make_a_projector() {
    let base = "class MmprojModel(ModelBase):\n    pass\n";
    let child = "@ModelBase.register(\"Gemma3ForConditionalGeneration\")\nclass Gemma3Vision(MmprojModel):\n    pass\n";
    let a = scan(&[child, base]);
    assert!(a.supports_vision("Gemma3ForConditionalGeneration"));
    assert!(!a.supports_text("Gemma3ForConditionalGeneration"));
}

#[test]
fn a_real_converter_folder_is_scanned() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("conversion")).unwrap();
    std::fs::write(dir.path().join("convert_hf_to_gguf.py"), "import sys\n").unwrap();
    std::fs::write(dir.path().join("conversion/llama.py"), SOURCE_A).unwrap();
    std::fs::write(
        dir.path().join("conversion/notes.txt"),
        "@ModelBase.register(\"X\")\nclass A(B):\n",
    )
    .unwrap();
    let a = scan_dir(dir.path()).unwrap();
    assert!(a.supports_text("Qwen3ForCausalLM"));
    assert!(!a.supports_text("X"));
    assert!(scan_dir(&dir.path().join("missing")).is_err());
}

fn siblings() -> Vec<serde_json::Value> {
    vec![
        json!({"rfilename": ".gitattributes", "size": 1519}),
        json!({"rfilename": "README.md", "size": 6772}),
        json!({"rfilename": "config.json", "size": 861}),
        json!({"rfilename": "generation_config.json", "size": 132}),
        json!({"rfilename": "merges.txt", "size": 466391}),
        json!({"rfilename": "model.safetensors", "size": 269060552,
               "lfs": {"sha256": "5AF571CB4D0B7D2A4B2DE3BF4B3B2A6A6A6A6A6A6A6A6A6A6A6A6A6A6A6A6A6A", "size": 269060552}}),
        json!({"rfilename": "onnx/model.onnx", "size": 540345794}),
        json!({"rfilename": "runs/x/events.out.tfevents", "size": 78157}),
        json!({"rfilename": "trainer_state.json", "size": 57225}),
        json!({"rfilename": "training_args.bin", "size": 6520}),
        json!({"rfilename": "tokenizer.json", "size": 2104556}),
        json!({"rfilename": "huge-table.json", "size": 400000000}),
    ]
}

#[test]
fn only_the_files_a_converter_reads_are_downloaded() {
    let files = select_files(&siblings());
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "config.json",
            "generation_config.json",
            "merges.txt",
            "model.safetensors",
            "tokenizer.json"
        ]
    );
    // The LFS digest is kept (lower case) and checked after the download.
    let weights = files
        .iter()
        .find(|f| f.name == "model.safetensors")
        .unwrap();
    assert_eq!(weights.size, 269_060_552);
    assert!(weights.sha256.as_deref().unwrap().starts_with("5af571cb"));
    assert_eq!(
        files
            .iter()
            .find(|f| f.name == "config.json")
            .unwrap()
            .sha256,
        None
    );
}

#[test]
fn mistral_consolidated_weights_are_not_downloaded_twice() {
    let sib = vec![
        json!({"rfilename": "config.json", "size": 10}),
        json!({"rfilename": "params.json", "size": 10}),
        json!({"rfilename": "consolidated.safetensors", "size": 1000}),
        json!({"rfilename": "model-00001-of-00002.safetensors", "size": 500}),
        json!({"rfilename": "model-00002-of-00002.safetensors", "size": 500}),
        json!({"rfilename": "model.safetensors.index.json", "size": 10}),
    ];
    let names: Vec<String> = select_files(&sib).into_iter().map(|f| f.name).collect();
    assert_eq!(
        names,
        vec![
            "config.json",
            "model-00001-of-00002.safetensors",
            "model-00002-of-00002.safetensors",
            "model.safetensors.index.json"
        ]
    );
    // Without the transformers layout the consolidated file is all there is.
    let only = vec![
        json!({"rfilename": "params.json", "size": 10}),
        json!({"rfilename": "consolidated.safetensors", "size": 1000}),
    ];
    assert_eq!(select_files(&only).len(), 2);
}

/// Shaped like the Hub record (`/api/models/<repo>?blobs=true`): its `config` has no
/// `vocab_size` / `hidden_size`; those come from the repo's `config.json` ([`smol_config`]).
fn smol_record() -> serde_json::Value {
    json!({
        "pipeline_tag": "text-generation",
        "config": {
            "architectures": ["LlamaForCausalLM"],
            "model_type": "llama",
            "tokenizer_config": {"eos_token": "<|im_end|>"}
        },
        "safetensors": {"total": 134515008},
        "cardData": {"license": "apache-2.0"},
        "gated": false,
        "siblings": siblings(),
    })
}

/// The repo's `config.json` (the fields the estimate reads, plus one the record also has).
fn smol_config() -> serde_json::Value {
    json!({
        "architectures": ["LlamaForCausalLM"],
        "model_type": "llama",
        "vocab_size": 49152,
        "hidden_size": 576,
        "intermediate_size": 1536,
        "num_hidden_layers": 30
    })
}

/// The record with the repo's `config.json` merged in, as `plan` builds it.
fn smol_info() -> serde_json::Value {
    let mut info = smol_record();
    merge_config(&mut info, smol_config());
    info
}

fn plan_for(
    info: &serde_json::Value,
    archs: Option<&arch::Architectures>,
    free: u64,
) -> ConvertPlan {
    build_plan(
        "HuggingFaceTB/SmolLM2-135M-Instruct",
        info,
        archs,
        free,
        PathBuf::from("models/.cache/convert"),
        false,
        Some("b11559".into()),
    )
}

#[test]
fn a_supported_repo_is_convertible_with_size_estimates() {
    let archs = scan(&[SOURCE_A]);
    let plan = plan_for(&smol_info(), Some(&archs), 50_000_000_000);
    assert!(plan.convertible, "{:?}", plan.reason);
    assert_eq!(plan.architecture.as_deref(), Some("LlamaForCausalLM"));
    assert_eq!(plan.license.as_deref(), Some("apache-2.0"));
    assert_eq!(plan.parameters, 134_515_008);
    assert!(!plan.vision);
    assert!(plan.architectures_checked);
    assert_eq!(plan.env_estimated_bytes, env::ESTIMATED_SIZE_BYTES);
    let q4 = plan.estimate("Q4_K_M");
    let f16 = plan.estimate("F16");
    // 134.5 M weights, 28.3 M of them the token embedding (kept at a higher precision):
    // the attention and gate / up matrices (rows of 576, not a multiple of 256) fall back to
    // Q5_0. Measured 105,453,984 bytes for Q4_K_M (b11342): within 3%; ~269 MB at 16 bits.
    assert_eq!(plan.embedding_parameters, 49_152 * 576);
    assert_eq!(
        plan.fallback_parameters,
        134_515_008 - 49_152 * 576 - 30 * 1536 * 576
    );
    assert!(
        (102_290_000..108_620_000).contains(&q4.size_bytes),
        "{}",
        q4.size_bytes
    );
    assert!(
        (268_000_000..270_000_000).contains(&f16.size_bytes),
        "{}",
        f16.size_bytes
    );
    // A 16-bit target needs no second copy: less disk than a quantized one.
    assert!(f16.disk_bytes < q4.disk_bytes + q4.size_bytes);
    assert!(q4.disk_bytes > plan.download_bytes + 2 * plan.parameters);
    assert_eq!(plan.quants.len(), CHOICES.len());
    assert_eq!(plan.default_quant, "Q4_K_M");
    plan.check("Q4_K_M").unwrap();
}

#[test]
fn the_size_estimate_needs_the_repo_config_the_hub_record_leaves_out() {
    let archs = scan(&[SOURCE_A]);
    // The Hub record alone: no embedding size, Q4_K_M ~81 MB against 105.5 MB measured.
    let bare = plan_for(&smol_record(), Some(&archs), 50_000_000_000);
    assert_eq!(bare.embedding_parameters, 0);
    assert!(bare.estimate("Q4_K_M").size_bytes < 90_000_000);
    // Merged: the record's fields stay, the missing ones come from `config.json`.
    let info = smol_info();
    assert_eq!(
        info["config"]["tokenizer_config"]["eos_token"],
        "<|im_end|>"
    );
    assert_eq!(info["config"]["num_hidden_layers"], 30);
    let plan = plan_for(&info, Some(&archs), 50_000_000_000);
    assert_eq!(plan.embedding_parameters, 49_152 * 576);
    assert!(plan.estimate("Q4_K_M").size_bytes > 94_900_000);
    // A record without any config gets one.
    // A hidden size that is a multiple of 256 (Llama-3.2-1B: 2048 / 8192): nothing falls back.
    let mut llama = smol_info();
    llama["config"]["hidden_size"] = json!(2048);
    llama["config"]["intermediate_size"] = json!(8192);
    let plan = plan_for(&llama, Some(&archs), 50_000_000_000);
    assert_eq!(plan.fallback_parameters, 0);
    let mut none = json!({"siblings": []});
    merge_config(&mut none, smol_config());
    assert_eq!(none["config"]["hidden_size"], 576);
}

#[test]
fn an_unknown_architecture_is_refused_with_its_name() {
    let archs = scan(&[SOURCE_A]);
    let mut info = smol_info();
    info["config"]["architectures"] = json!(["BrandNewForCausalLM"]);
    let plan = plan_for(&info, Some(&archs), u64::MAX);
    assert!(!plan.convertible);
    let reason = plan.reason.clone().unwrap();
    assert!(
        reason.starts_with("convert.unsupported_architecture"),
        "{reason}"
    );
    assert!(reason.contains("BrandNewForCausalLM"));
    assert_eq!(plan.check("Q4_K_M").unwrap_err(), reason);
    // Without the converter's list only the task is checked.
    let unchecked = plan_for(&info, None, u64::MAX);
    assert!(unchecked.convertible && !unchecked.architectures_checked);
}

#[test]
fn repos_that_are_not_llms_or_not_full_models_are_refused() {
    let archs = scan(&[SOURCE_A]);
    let reason = |info: &serde_json::Value| {
        plan_for(info, Some(&archs), u64::MAX)
            .reason
            .unwrap_or_default()
    };

    let mut embedder = smol_info();
    embedder["pipeline_tag"] = json!("feature-extraction");
    assert!(
        reason(&embedder).starts_with("convert.not_llm"),
        "{}",
        reason(&embedder)
    );

    let mut no_weights = smol_info();
    no_weights["siblings"] = json!([{"rfilename": "config.json", "size": 10}]);
    assert!(reason(&no_weights).starts_with("convert.no_safetensors"));

    let mut adapter = smol_info();
    adapter["siblings"] = json!([
        {"rfilename": "adapter_config.json", "size": 10},
        {"rfilename": "adapter_model.safetensors", "size": 1000}
    ]);
    assert!(reason(&adapter).starts_with("convert.adapter_only"));

    let mut no_arch = smol_info();
    no_arch["config"] = json!({});
    assert!(reason(&no_arch).starts_with("convert.no_architecture"));
}

#[test]
fn a_vision_language_model_also_gets_a_projector() {
    let archs = scan(&[SOURCE_A]);
    let mut info = smol_info();
    info["pipeline_tag"] = json!("image-text-to-text");
    info["config"]["architectures"] = json!(["Idefics3ForConditionalGeneration"]);
    let plan = plan_for(&info, Some(&archs), u64::MAX);
    assert!(plan.convertible && plan.vision);
    // A text-only model of the same task is converted without one.
    let mut text_only = smol_info();
    text_only["pipeline_tag"] = json!("image-text-to-text");
    assert!(!plan_for(&text_only, Some(&archs), u64::MAX).vision);
}

#[test]
fn not_enough_disk_is_reported_with_the_numbers() {
    let archs = scan(&[SOURCE_A]);
    let plan = plan_for(&smol_info(), Some(&archs), 100_000_000);
    let e = plan.check("Q4_K_M").unwrap_err();
    assert!(e.starts_with("convert.not_enough_disk"), "{e}");
    assert!(e.contains("MB"), "{e}");
    // The estimate grows with the work: no parameter count falls back to the file size / 2.
    let mut info = smol_info();
    info["safetensors"] = json!(null);
    assert_eq!(
        plan_for(&info, Some(&archs), u64::MAX).parameters,
        269_060_552 / 2
    );
}

#[test]
fn disk_estimates_follow_the_stages() {
    let q = estimate(1_000_000_000, 0, 2_000_000_000, "Q8_0");
    // 2 GB download + 2 GB 16-bit + 1.06 GB result, plus margin and floor.
    assert!(q.size_bytes > 1_000_000_000 && q.size_bytes < 1_100_000_000);
    assert!(
        q.disk_bytes > 5_000_000_000 && q.disk_bytes < 6_000_000_000,
        "{}",
        q.disk_bytes
    );
    assert!(estimate(1_000_000_000, 0, 2_000_000_000, "F16").disk_bytes < q.disk_bytes);
    // An unknown type is estimated like Q8_0 (a safe upper guess).
    assert_eq!(bits_per_weight("IQ4_XS"), 8.5);
}

#[test]
fn quantization_names_are_normalized() {
    assert_eq!(normalize_quant(" q4_k_m ").as_deref(), Some("Q4_K_M"));
    assert_eq!(normalize_quant("f16").as_deref(), Some("F16"));
    assert_eq!(normalize_quant(""), None);
    assert_eq!(normalize_quant("Q4 K"), None);
    assert_eq!(normalize_quant("../x"), None);
    assert!(is_direct("F16") && is_direct("BF16") && !is_direct("Q4_K_M"));
}

#[test]
fn results_land_in_the_gguf_folder_of_the_repo() {
    let models = PathBuf::from("models");
    assert_eq!(
        target_dir_in(&models, "Qwen/Qwen3-0.6B"),
        PathBuf::from("models/Qwen/Qwen3-0.6B-GGUF")
    );
    assert_eq!(
        output_name("Qwen/Qwen3-0.6B", "Q4_K_M"),
        "Qwen3-0.6B-Q4_K_M.gguf"
    );
    assert_eq!(
        mmproj_name("HuggingFaceTB/SmolVLM-256M-Instruct"),
        "mmproj-SmolVLM-256M-Instruct-F16.gguf"
    );
}

#[test]
fn converter_output_lines_become_progress() {
    assert_eq!(
        parse_line("Writing:  21%|██        | 56.6M/269M [00:00<00:00, 384Mbyte/s]"),
        Line::Bar(0.21)
    );
    assert_eq!(
        parse_line("Writing: 100%|██████████| 269M/269M [00:00<00:00, 416Mbyte/s]"),
        Line::Bar(1.0)
    );
    assert_eq!(
        parse_line("INFO:hf-to-gguf:gguf: loading model weight map from 'model.safetensors'"),
        Line::Info("gguf: loading model weight map from 'model.safetensors'".into())
    );
    assert_eq!(
        parse_line("INFO:gguf.vocab:Adding 48900 merge(s)."),
        Line::Other
    );
    assert_eq!(parse_line(""), Line::Other);

    assert_eq!(
        parse_quantize_step("[ 271/ 272]       blk.29.ffn_norm.weight  - [   576,  1,  1,  1], type =    f32, size ="),
        Some((271, 272))
    );
    assert_eq!(
        parse_quantize_step("[   1/ 272]   output.weight"),
        Some((1, 272))
    );
    assert_eq!(
        parse_quantize_step("llama_model_quantize_impl: model size = 256 MiB"),
        None
    );
}

#[test]
fn a_refused_architecture_is_found_in_the_converter_log() {
    let log = "Traceback (most recent call last):\n  File \"convert_hf_to_gguf.py\", line 1\nNotImplementedError: Architecture 'FooForCausalLM' not supported!";
    assert_eq!(
        unsupported_architecture_in(log).as_deref(),
        Some("FooForCausalLM")
    );
    assert_eq!(
        unsupported_architecture_in("RuntimeError: out of memory"),
        None
    );
}

#[test]
fn release_tags_are_the_only_thing_that_reaches_a_url() {
    assert!(env::valid_tag("b11559"));
    assert!(env::valid_tag("b123"));
    for bad in [
        "",
        "11559",
        "b",
        "b11559/../x",
        "b1155 9",
        "B11559",
        "b11559\n",
        "master",
        "b99999999",
    ] {
        assert!(!env::valid_tag(bad), "{bad:?}");
    }
}

#[test]
fn converter_files_are_filtered_out_of_the_source_archive() {
    for yes in [
        "convert_hf_to_gguf.py",
        "convert_lora_to_gguf.py",
        "conversion/llama.py",
        "gguf-py/gguf/constants.py",
        "requirements/requirements-convert_hf_to_gguf.txt",
    ] {
        assert!(env::is_converter_path(yes), "{yes}");
    }
    for no in ["src/llama.cpp", "ggml/src/ggml.c", "conversion"] {
        assert!(!env::is_converter_path(no), "{no}");
    }

    // A tag archive with the converter and a lot else: only the converter comes out.
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("src.tar.gz");
    {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        for (path, body) in [
            ("llama.cpp-b1/convert_hf_to_gguf.py", "print('convert')"),
            ("llama.cpp-b1/convert_lora_to_gguf.py", "print('adapter')"),
            ("llama.cpp-b1/conversion/__init__.py", ""),
            ("llama.cpp-b1/conversion/llama.py", "# llama"),
            (
                "llama.cpp-b1/requirements/requirements-convert_hf_to_gguf.txt",
                "torch==2.11.0",
            ),
            ("llama.cpp-b1/gguf-py/gguf/__init__.py", ""),
            ("llama.cpp-b1/src/llama.cpp", "int main() {}"),
            ("llama.cpp-b1/models/ggml-vocab-llama.gguf", "binary"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, body.as_bytes()).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }
    let out = dir.path().join("out");
    env::extract_converter(&archive, &out).unwrap();
    assert!(out.join("convert_hf_to_gguf.py").is_file());
    assert!(out.join("convert_lora_to_gguf.py").is_file());
    assert!(out.join("conversion/llama.py").is_file());
    assert!(out.join("gguf-py/gguf/__init__.py").is_file());
    assert!(!out.join("src").exists() && !out.join("models").exists());

    // An archive without the converter is not a usable source.
    let empty = dir.path().join("empty.tar.gz");
    {
        let gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&empty).unwrap(),
            flate2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "llama.cpp-b1/README.md", "x".as_bytes())
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    assert!(env::extract_converter(&empty, &dir.path().join("out2")).is_err());
}

#[test]
fn the_environment_follows_the_requirements_of_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let req = dir.path().join("requirements");
    std::fs::create_dir_all(&req).unwrap();
    std::fs::write(
        req.join("requirements-convert_hf_to_gguf.txt"),
        "torch==2.11.0\n",
    )
    .unwrap();
    std::fs::write(
        req.join("requirements-convert_legacy_llama.txt"),
        "numpy~=2.2.6\n",
    )
    .unwrap();
    let a = env::requirements_digest(dir.path()).unwrap();
    assert_eq!(a, env::requirements_digest(dir.path()).unwrap());
    std::fs::write(
        req.join("requirements-convert_hf_to_gguf.txt"),
        "torch==2.12.0\n",
    )
    .unwrap();
    assert_ne!(a, env::requirements_digest(dir.path()).unwrap());
    assert!(env::requirements_digest(&dir.path().join("none")).is_err());
}

#[test]
fn environment_status_reflects_what_is_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("convert-env");
    let none = env::status_in(&root, Some("b11559"));
    assert!(!none.installed && !none.ready && none.size_bytes == 0);

    // A finished environment of another tag is installed but not ready for this runtime.
    std::fs::create_dir_all(env::python_bin_in(&root).parent().unwrap()).unwrap();
    std::fs::write(env::python_bin_in(&root), b"python").unwrap();
    std::fs::create_dir_all(env::source_dir_in(&root, "b11559")).unwrap();
    std::fs::write(
        env::source_dir_in(&root, "b11559").join("convert_hf_to_gguf.py"),
        "x",
    )
    .unwrap();
    let manifest = env::Manifest {
        uv_version: env::UV_VERSION.into(),
        python: "Python 3.12.13".into(),
        tag: "b11236".into(),
        requirements_sha256: "00".into(),
        installed_at: 1,
    };
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let old = env::status_in(&root, Some("b11559"));
    assert!(old.installed && !old.ready);
    assert!(old.size_bytes > 0);
    let mut current = manifest.clone();
    current.tag = "b11559".into();
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&current).unwrap(),
    )
    .unwrap();
    assert!(env::status_in(&root, Some("b11559")).ready);
    assert!(!env::status_in(&root, None).ready);

    env::remove_in(&root).unwrap();
    assert!(!root.exists());
    env::remove_in(&root).unwrap();
}

#[test]
fn the_runtime_tag_comes_from_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(runtime_tag_in(dir.path()), None);
    std::fs::write(
        dir.path().join(crate::runtime::RUNTIME_MANIFEST),
        r#"{"version": "b11559", "backend": "vulkan"}"#,
    )
    .unwrap();
    assert_eq!(runtime_tag_in(dir.path()).as_deref(), Some("b11559"));
    std::fs::write(
        dir.path().join(crate::runtime::RUNTIME_MANIFEST),
        r#"{"version": "../evil"}"#,
    )
    .unwrap();
    assert_eq!(runtime_tag_in(dir.path()), None);
}

#[test]
fn job_status_helpers() {
    assert!(ConvertStatus::Queued.is_active() && ConvertStatus::Running.is_active());
    assert!(!ConvertStatus::Completed.is_active());
    assert!(!ConvertStatus::Failed.is_active() && !ConvertStatus::Cancelled.is_active());
    assert_eq!(
        serde_json::to_string(&ConvertStage::Quantizing).unwrap(),
        "\"quantizing\""
    );
    assert!(job(u64::MAX).is_none());
    assert!(cancel(u64::MAX)
        .unwrap_err()
        .starts_with("convert.job_not_found"));
    assert!(remove(u64::MAX)
        .unwrap_err()
        .starts_with("convert.job_not_found"));
}

#[tokio::test]
async fn a_process_is_followed_line_by_line() {
    // The test binary itself is the helper process: `--list` prints one line per test.
    let exe = std::env::current_exe().unwrap();
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(["--list", "--format", "terse"]);
    let cancel = Cancel::default();
    let mut lines = Vec::new();
    let outcome = proc::run(cmd, &cancel.flag(), |l| lines.push(l.to_string()))
        .await
        .unwrap();
    assert!(outcome.success && !outcome.cancelled);
    assert!(
        lines.iter().any(|l| l.contains("convert::tests::")),
        "{lines:?}"
    );
    // The tail keeps the last lines, bars left out.
    assert!(!outcome.tail.is_empty() && outcome.tail.len() <= 40);
    assert!(proc::is_bar_line("Writing:  21%|██ | 56M/269M"));
    assert!(!proc::is_bar_line(
        "INFO:hf-to-gguf:Model successfully exported"
    ));

    // A failing process reports its exit code and what it said.
    let mut failing = tokio::process::Command::new(&exe);
    failing.args(["--no-such-flag-at-all"]);
    let outcome = proc::run(failing, &Cancel::default().flag(), |_| {})
        .await
        .unwrap();
    assert!(!outcome.success && outcome.code.is_some());
    assert!(!outcome.log().is_empty());

    // A program that is not there is an error, not a hang.
    let missing = tokio::process::Command::new(exe.with_file_name("no-such-program-here"));
    assert!(proc::run(missing, &Cancel::default().flag(), |_| {})
        .await
        .is_err());
}

/// Helper process of the cancellation test below (never runs on its own).
#[test]
#[ignore = "child process of a_running_process_is_killed_on_cancel"]
fn sleeper() {
    println!("sleeper started");
    std::thread::sleep(std::time::Duration::from_secs(60));
}

#[tokio::test]
async fn a_running_process_is_killed_on_cancel() {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args([
        "--ignored",
        "--exact",
        "convert::tests::sleeper",
        "--nocapture",
    ]);
    let cancel = Cancel::default();
    let flag = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        flag.set();
    });
    let started = std::time::Instant::now();
    let outcome = proc::run(cmd, &cancel.flag(), |_| {}).await.unwrap();
    assert!(outcome.cancelled && !outcome.success);
    assert!(started.elapsed() < std::time::Duration::from_secs(30));
}

/// Reads a real llama.cpp checkout / converter folder (`HS_CONVERTER_SRC`, for example the
/// `src/<tag>` folder of the conversion environment).
#[test]
#[ignore = "needs HS_CONVERTER_SRC"]
fn the_real_converter_registers_common_architectures() {
    let Some(dir) = std::env::var_os("HS_CONVERTER_SRC").map(PathBuf::from) else {
        return;
    };
    let a = scan_dir(&dir).unwrap();
    eprintln!(
        "{} text, {} vision architectures",
        a.text.len(),
        a.vision.len()
    );
    if let Some(out) = std::env::var_os("HS_CONVERTER_DUMP") {
        let list = |s: &std::collections::BTreeSet<String>| {
            s.iter().cloned().collect::<Vec<_>>().join(
                "
",
            )
        };
        std::fs::write(PathBuf::from(&out).join("rs_text.txt"), list(&a.text)).unwrap();
        std::fs::write(PathBuf::from(&out).join("rs_vis.txt"), list(&a.vision)).unwrap();
    }
    for name in [
        "LlamaForCausalLM",
        "Qwen3ForCausalLM",
        "Gemma3ForCausalLM",
        "MistralForCausalLM",
    ] {
        assert!(a.supports_text(name), "{name}");
    }
    assert!(a.text.len() > 100, "{}", a.text.len());
    assert!(!a.vision.is_empty());
    assert!(!a.supports_text("TotallyNewForCausalLM"));
    eprintln!("quant methods: {:?}", a.quant_methods);
    assert!(a.supports_quant("gptq") && a.supports_quant("fp8"));
    assert!(!a.supports_quant("awq") && !a.supports_quant("bitsandbytes"));
}

/// The dequantizing branches of the converter (`ModelBase.dequant_model`, b11583).
const SOURCE_QUANT: &str = r#"
            quant_method = quant_config.get("quant_method")
            if quant_method == "bitnet":
                pass
            elif quant_method == "fp8":
                pass
            elif quant_method == "gptq":
                pass
            elif quant_method == "compressed-tensors":
                pass
        self._is_mxfp4 = quant_method == "mxfp4"
"#;

#[test]
fn the_converter_source_lists_the_quant_methods_it_dequantizes() {
    let archs = scan(&[SOURCE_A, SOURCE_QUANT]);
    for m in ["bitnet", "fp8", "gptq", "compressed-tensors", "mxfp4"] {
        assert!(archs.supports_quant(m), "{m}");
    }
    assert!(archs.supports_quant("GPTQ"));
    assert!(!archs.supports_quant("awq"));
    assert!(!archs.supports_quant("bitsandbytes"));
    // A converter source without any branch refuses nothing.
    assert!(scan(&[SOURCE_A]).supports_quant("awq"));
}

#[test]
fn a_checkpoint_quantized_with_an_unreadable_method_is_refused_before_download() {
    let archs = scan(&[SOURCE_A, SOURCE_QUANT]);
    let mut info = smol_info();
    info["config"]["quantization_config"] = json!({"quant_method": "awq", "bits": 4});
    info["cardData"]["base_model"] = json!(["HuggingFaceTB/SmolLM2-135M"]);
    let plan = plan_for(&info, Some(&archs), u64::MAX);
    assert!(!plan.convertible);
    assert_eq!(plan.quant_method.as_deref(), Some("awq"));
    assert_eq!(
        plan.base_model.as_deref(),
        Some("HuggingFaceTB/SmolLM2-135M")
    );
    let reason = plan.reason.clone().unwrap();
    assert!(
        reason.starts_with("convert.unsupported_quant_base"),
        "{reason}"
    );
    assert!(reason.contains("awq") && reason.contains("HuggingFaceTB/SmolLM2-135M"));
    assert_eq!(plan.check("Q4_K_M").unwrap_err(), reason);

    // Without a card: the base model comes from the tags, else it is not named.
    info["cardData"] = json!({"license": "apache-2.0"});
    info["tags"] = json!(["base_model:quantized:HuggingFaceTB/SmolLM2-135M-Instruct"]);
    let plan = plan_for(&info, Some(&archs), u64::MAX);
    assert_eq!(
        plan.base_model.as_deref(),
        Some("HuggingFaceTB/SmolLM2-135M-Instruct")
    );
    info["tags"] = json!([]);
    let reason = plan_for(&info, Some(&archs), u64::MAX).reason.unwrap();
    assert!(
        reason.starts_with("convert.unsupported_quant {"),
        "{reason}"
    );

    // A method the converter dequantizes (also under text_config) stays convertible.
    let mut fp8 = smol_info();
    fp8["config"]["text_config"] = json!({"quantization_config": {"quant_method": "FP8"}});
    let plan = plan_for(&fp8, Some(&archs), u64::MAX);
    assert!(plan.convertible, "{:?}", plan.reason);
    assert_eq!(plan.quant_method.as_deref(), Some("fp8"));
}
