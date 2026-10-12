use super::*;
use crate::domain::store::Domain;
use crate::params::ParamValue;

const QWEN_GGUF: &str =
    r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf";

const ASSISTANT: &str = include_str!("../tests/data/modelfile/qwen3-4b-assistant.Modelfile");
const LORA: &str = include_str!("../tests/data/modelfile/qwen3-4b-lora.Modelfile");
const MARIO: &str = include_str!("../tests/data/modelfile/mario.Modelfile");
const SAFETENSORS: &str = include_str!("../tests/data/modelfile/safetensors-dir.Modelfile");

fn has(w: &ModelfileImport, code: &str) -> bool {
    w.warnings.iter().any(|x| x.code == code)
}

/// A minimal but real Ollama models directory: manifest JSON + content-addressed blobs
/// (the model blob starts with the GGUF magic).
fn ollama_dir_with_tiny() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let blobs = dir.path().join("blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    let d = |c: char| c.to_string().repeat(64);
    std::fs::write(blobs.join(format!("sha256-{}", d('a'))), b"GGUF\x03\0\0\0").unwrap();
    std::fs::write(
        blobs.join(format!("sha256-{}", d('b'))),
        r#"{"temperature":0.6,"top_k":20,"stop":["<|im_end|>"],"num_ctx":2048}"#,
    )
    .unwrap();
    std::fs::write(
        blobs.join(format!("sha256-{}", d('c'))),
        "You are the stored system prompt.",
    )
    .unwrap();
    std::fs::write(blobs.join(format!("sha256-{}", d('d'))), "{{ .Prompt }}").unwrap();
    let manifest = format!(
        r#"{{"schemaVersion":2,"layers":[
        {{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{a}","size":8}},
        {{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:{dd}","size":13}},
        {{"mediaType":"application/vnd.ollama.image.system","digest":"sha256:{c}","size":33}},
        {{"mediaType":"application/vnd.ollama.image.params","digest":"sha256:{b}","size":70}}]}}"#,
        a = d('a'),
        b = d('b'),
        c = d('c'),
        dd = d('d')
    );
    let mdir = dir.path().join("manifests/registry.ollama.ai/library/tiny");
    std::fs::create_dir_all(&mdir).unwrap();
    std::fs::write(mdir.join("latest"), manifest).unwrap();
    dir
}

#[test]
fn syntax_comments_case_quotes_and_multiline() {
    let items = parse("# c\r\nfrom  a.gguf\r\nSystem \"\"\"\nline1\nline2\n\"\"\"\nPARAMETER stop \"</s>\"\nTEMPLATE \"\"\"{{ .Prompt }}\"\"\"\n").unwrap();
    let keys: Vec<_> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["from", "system", "parameter", "template"]);
    assert_eq!(items[0].value, "a.gguf");
    assert_eq!(items[1].value, "line1\nline2");
    assert_eq!(items[1].line, 3);
    assert_eq!(items[2].value, "stop \"</s>\"");
    assert_eq!(items[3].value, "{{ .Prompt }}");
    let err = parse("FROM x\nSYSTEM \"\"\"never closed\n").unwrap_err();
    let (key, params) = crate::error_key::parse(&err).unwrap();
    assert_eq!(key, "modelfile.unclosed_block", "{err}");
    assert_eq!(params["line"], 2);
    assert_eq!(params["instruction"], "SYSTEM");
}

#[test]
fn official_style_modelfile_maps_every_field() {
    let r = import_text(ASSISTANT, &ImportOptions::default()).unwrap();
    let def = &r.import.definition;
    assert_eq!(def.base.local_path().unwrap(), Path::new(QWEN_GGUF));
    assert_eq!(r.import.suggested_alias, "Qwen3-4B-Instruct-2507-Q4_K_M");
    assert_eq!(def.load.get("context_size"), Some(&ParamValue::Int(8192)));
    let p = r.import.preset.as_ref().unwrap();
    let s = &p.sampling;
    assert_eq!(s.temperature, Some(0.7));
    assert_eq!(s.top_p, Some(0.8));
    assert_eq!(s.top_k, Some(20));
    assert_eq!(s.min_p, Some(0.0));
    assert_eq!(s.repeat_penalty, Some(1.05));
    assert_eq!(s.max_tokens, Some(256));
    assert_eq!(s.seed, Some(42));
    assert_eq!(s.stop, ["<|im_start|>", "<|im_end|>"]);
    let sys = p.system_prompt.as_deref().unwrap();
    // SYSTEM first, then the four MESSAGE turns as an example conversation.
    assert!(sys.starts_with("You are Mario"));
    assert!(sys.contains(
        "word Mamma-mia.

Example conversation"
    ));
    assert!(sys.ends_with(
        "User: Is Sacramento in Canada?

Assistant: no"
    ));
    assert!(sys.contains(
        "User: Is Toronto in Canada?

Assistant: yes

User: Is Sac"
    ));
    assert!(has(&r, "template_ignored") || has(&r, "template_ignored_no_jinja"));
    let messages = r
        .warnings
        .iter()
        .find(|w| w.code == "messages_as_examples")
        .unwrap();
    assert_eq!(messages.detail, "4");
    assert!(r
        .mapped
        .iter()
        .any(|m| m.target == "system_prompt.examples" && m.value == "4"));
    assert!(!r.warnings.iter().any(|w| w.code.contains("unknown")));
    assert!(r
        .mapped
        .iter()
        .any(|m| m.target == "load.context_size" && m.value == "8192"));
}

#[test]
fn lora_load_params_and_warnings() {
    let dir = tempfile::tempdir().unwrap();
    let lora = dir.path().join("style-lora.gguf");
    std::fs::write(&lora, b"GGUF\x03\0\0\0").unwrap();
    let r = import_text(
        LORA,
        &ImportOptions {
            base_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        },
    )
    .unwrap();
    let def = &r.import.definition;
    assert_eq!(def.adapters.len(), 1);
    assert!(def.adapters[0].path.ends_with("style-lora.gguf"));
    assert!(r.import.missing.is_empty());
    assert_eq!(def.load.get("gpu_layers"), Some(&ParamValue::Int(99)));
    assert_eq!(def.load.get("threads"), Some(&ParamValue::Int(8)));
    assert_eq!(def.load.get("logical_batch"), Some(&ParamValue::Int(512)));
    let p = r.import.preset.as_ref().unwrap();
    assert_eq!(p.sampling.mirostat, Some(2));
    assert_eq!(p.sampling.mirostat_tau, Some(5.0));
    assert_eq!(p.sampling.mirostat_eta, Some(0.1));
    assert_eq!(p.sampling.max_tokens, None, "num_predict -1 = unlimited");
    assert_eq!(
        p.system_prompt.as_deref(),
        Some("You are a terse assistant.")
    );
    let ignored: Vec<_> = r
        .warnings
        .iter()
        .filter(|w| w.code == "ignored_parameter")
        .map(|w| w.detail.split(' ').next().unwrap())
        .collect();
    assert_eq!(ignored, ["tfs_z", "num_keep", "penalize_newline"]);
    let unknown = r
        .warnings
        .iter()
        .find(|w| w.code == "unknown_parameter")
        .unwrap();
    assert_eq!(unknown.detail, "not_a_real_option 3");
    assert!(has(&r, "ignored_instruction"), "REQUIRES");
}

#[test]
fn missing_adapter_and_model_are_reported_not_fatal() {
    let r = import_text(
        "FROM ./nope/model.gguf\nADAPTER ./nope/lora.gguf\n",
        &ImportOptions::default(),
    )
    .unwrap();
    assert!(!r.import.model_found);
    assert!(has(&r, "model_file_missing"));
    assert_eq!(r.import.missing, ["./nope/lora.gguf"]);
    assert!(r.import.preset.is_none());
}

#[test]
fn invalid_values_and_ranges() {
    let r = import_text(
        "FROM ./x.gguf\nPARAMETER temperature hot\nPARAMETER mirostat 5\nPARAMETER num_ctx -3\nPARAMETER top_k\n",
        &ImportOptions::default(),
    )
    .unwrap();
    assert_eq!(
        r.warnings
            .iter()
            .filter(|w| w.code == "invalid_value")
            .count(),
        4
    );
    let err = import_text(
        "FROM ./x.gguf\nPARAMETER temperature 9\n",
        &ImportOptions::default(),
    )
    .unwrap_err();
    assert!(err.contains("temperature"), "{err}");
    assert!(import_text("PARAMETER temperature 1\n", &ImportOptions::default()).is_err());
}

#[test]
fn directory_from_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Qwen3-4B-hf")).unwrap();
    let err = import_text(
        SAFETENSORS,
        &ImportOptions {
            base_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        },
    )
    .unwrap_err();
    let (key, _) = crate::error_key::parse(&err).unwrap();
    assert_eq!(key, "modelfile.from_is_dir", "{err}");
}

#[test]
fn wildcard_from_picks_first_shard() {
    let dir = tempfile::tempdir().unwrap();
    for n in ["m-00002-of-00002.gguf", "m-00001-of-00002.gguf"] {
        std::fs::write(dir.path().join(n), b"GGUF\x03\0\0\0").unwrap();
    }
    let r = import_text(
        "FROM ./m-*.gguf\n",
        &ImportOptions {
            base_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(r.import.model_found);
    assert!(r
        .import
        .definition
        .base
        .local_path()
        .unwrap()
        .ends_with("m-00001-of-00002.gguf"));
}

#[test]
fn ollama_model_name_resolves_blob_and_inherits_layers() {
    let ollama = ollama_dir_with_tiny();
    let opts = ImportOptions {
        ollama_dir: Some(ollama.path().to_path_buf()),
        ..Default::default()
    };
    let r = import_text(MARIO, &opts).unwrap();
    let def = &r.import.definition;
    assert!(r.import.model_found);
    let blob = def.base.local_path().unwrap();
    assert!(blob.starts_with(ollama.path().join("blobs")));
    // The Modelfile overrides the stored num_ctx / temperature / system; stop is inherited.
    assert_eq!(def.load.get("context_size"), Some(&ParamValue::Int(4096)));
    let p = r.import.preset.unwrap();
    assert_eq!(p.sampling.temperature, Some(1.0));
    assert_eq!(p.sampling.top_k, Some(20));
    assert_eq!(p.sampling.stop, ["<|im_end|>"]);
    assert_eq!(
        p.system_prompt.as_deref(),
        Some("You are Mario from super mario bros, acting as an assistant.")
    );
    assert_eq!(r.import.suggested_alias, "tiny");

    // `tiny:latest` is the same manifest; the stored system prompt applies when not overridden.
    let r = import_text("FROM tiny:latest\n", &opts).unwrap();
    assert_eq!(
        r.import.preset.unwrap().system_prompt.as_deref(),
        Some("You are the stored system prompt.")
    );
    let err = import_text("FROM nothing:7b\n", &opts).unwrap_err();
    let (key, params) = crate::error_key::parse(&err).unwrap();
    assert_eq!(key, "modelfile.ollama_model_missing", "{err}");
    assert_eq!(params["name"], "nothing:7b");
    assert!(import_text("FROM a/../b\n", &opts).is_err());
}

#[test]
fn manifest_names() {
    assert_eq!(
        manifest_parts("qwen3:30b-a3b").unwrap(),
        ["registry.ollama.ai", "library", "qwen3", "30b-a3b"]
    );
    assert_eq!(
        manifest_parts("user/model").unwrap(),
        ["registry.ollama.ai", "user", "model", "latest"]
    );
    assert_eq!(
        manifest_parts("hf.co/bartowski/Qwen-GGUF:Q4_K_M").unwrap(),
        ["hf.co", "bartowski", "Qwen-GGUF", "Q4_K_M"]
    );
    assert!(manifest_parts("a/../b").is_err());
}

#[test]
fn saves_through_the_domain_store() {
    let data = tempfile::tempdir().unwrap();
    let domain = Domain::at(data.path());
    let r = import_text(ASSISTANT, &ImportOptions::default()).unwrap();
    let saved = domain
        .import_definition(r.import.clone(), "mario-qwen")
        .unwrap();
    assert_eq!(saved.alias, "mario-qwen");
    let preset = domain
        .inference_preset(saved.default_preset_id.as_deref().unwrap())
        .unwrap();
    assert_eq!(preset.sampling.stop.len(), 2);
    assert!(preset.system_prompt.unwrap().contains("Mamma-mia"));
    let again = domain.import_definition(r.import, "mario-qwen");
    assert!(again.is_err(), "alias is unique");
}

/// Needs the real model file of this machine.
#[test]
#[ignore]
fn real_gguf_is_found_and_template_kept_from_the_gguf() {
    if !Path::new(QWEN_GGUF).is_file() {
        eprintln!("{QWEN_GGUF} missing, skipping");
        return;
    }
    let r = import_text(ASSISTANT, &ImportOptions::default()).unwrap();
    assert!(r.import.model_found);
    assert!(
        has(&r, "template_ignored"),
        "Qwen3 GGUF carries a Jinja template"
    );
}

/// Reads the user's real Ollama directory (read-only): resolves an installed model and checks
/// nothing under it changed.
#[test]
#[ignore]
fn real_ollama_model_resolves_read_only() {
    let Some(dir) = ollama_dir() else {
        eprintln!("no Ollama models directory, skipping");
        return;
    };
    let stamp = |p: &Path| {
        std::fs::metadata(p)
            .map(|m| (m.len(), m.modified().ok()))
            .ok()
    };
    let manifest = dir.join("manifests/registry.ollama.ai/library/qwen3/30b-a3b");
    if !manifest.is_file() {
        eprintln!("qwen3:30b-a3b not installed, skipping");
        return;
    }
    let before = stamp(&manifest);
    let r = import_text(
        "FROM qwen3:30b-a3b\nPARAMETER num_ctx 8192\n",
        &ImportOptions::default(),
    )
    .unwrap();
    let blob = r.import.definition.base.local_path().unwrap().to_path_buf();
    eprintln!("blob: {} ({} mapped)", blob.display(), r.mapped.len());
    assert!(blob.starts_with(dir.join("blobs")) && is_gguf(&blob));
    let p = r.import.preset.unwrap();
    assert_eq!(p.sampling.temperature, Some(0.6), "stored params inherited");
    assert_eq!(p.sampling.top_k, Some(20));
    assert_eq!(p.sampling.stop, ["<|im_start|>", "<|im_end|>"]);
    assert_eq!(stamp(&manifest), before);
}

/// The GUI's "import from an installed Ollama model" list: every listed model resolves with
/// `FROM <name>` to its GGUF blob (real Ollama directory, read-only).
#[test]
#[ignore]
fn real_ollama_installed_list_resolves() {
    let (dir, list) = crate::importer::scan_ollama_installed();
    let Some(dir) = dir else {
        eprintln!("no Ollama models directory, skipping");
        return;
    };
    eprintln!("{} GGUF models in {}", list.len(), dir.display());
    for m in &list {
        let r = import_text(&format!("FROM {}\n", m.name), &ImportOptions::default())
            .unwrap_or_else(|e| panic!("{}: {e}", m.name));
        let blob = r.import.definition.base.local_path().unwrap().to_path_buf();
        eprintln!(
            "{} -> {} ({} mapped)",
            m.name,
            blob.display(),
            r.mapped.len()
        );
        assert_eq!(blob, m.path);
        assert!(r.import.model_found);
    }
}

#[test]
fn message_syntax_roles_multiline_and_inherited_examples() {
    let text = "FROM C:\\x\\m.gguf\nMESSAGE user \"\"\"line one\nline two\"\"\"\nMESSAGE assistant \"quoted\"\nMESSAGE system be brief\nMESSAGE robot hi\nMESSAGE user\n";
    // Multi-line text after the role, a quoted single line, system role, a bad role, no text.
    let items = parse(text).unwrap();
    let msgs: Vec<&str> = items
        .iter()
        .filter(|i| i.key == "message")
        .map(|i| i.value.as_str())
        .collect();
    assert_eq!(msgs.len(), 5);
    assert_eq!(msgs[0], "user line one\nline two");
    let r = import_text(text, &ImportOptions::default()).unwrap();
    let sys = r.import.preset.unwrap().system_prompt.unwrap();
    assert_eq!(
        sys,
        "Example conversation (from the Modelfile MESSAGE instructions); answer in the same style:\n\nUser: line one\nline two\n\nAssistant: quoted\n\nSystem: be brief"
    );
    // Unknown role / missing text are reported, not silently dropped.
    let bad = r
        .warnings
        .iter()
        .filter(|w| w.code == "invalid_value")
        .count();
    assert_eq!(bad, 2, "{:?}", r.warnings);

    // An installed Ollama model's stored messages come first, then the Modelfile's own.
    let dir = ollama_dir_with_messages();
    let r = import_text(
        "FROM tiny\nSYSTEM base\nMESSAGE user own question\n",
        &ImportOptions {
            ollama_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        },
    )
    .unwrap();
    let sys = r.import.preset.unwrap().system_prompt.unwrap();
    assert!(sys.starts_with("base\n\nExample conversation"), "{sys}");
    assert!(
        sys.ends_with("User: stored question\n\nAssistant: stored answer\n\nUser: own question"),
        "{sys}"
    );
    assert!(r
        .warnings
        .iter()
        .any(|w| w.code == "messages_as_examples" && w.detail == "3"));
}

fn ollama_dir_with_messages() -> tempfile::TempDir {
    let dir = ollama_dir_with_tiny();
    let blobs = dir.path().join("blobs");
    let digest = "e".repeat(64);
    std::fs::write(
        blobs.join(format!("sha256-{digest}")),
        r#"[{"role":"user","content":"stored question"},{"role":"assistant","content":"stored answer"}]"#,
    )
    .unwrap();
    let mpath = dir
        .path()
        .join("manifests/registry.ollama.ai/library/tiny/latest");
    let mut m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&mpath).unwrap()).unwrap();
    m["layers"].as_array_mut().unwrap().push(serde_json::json!(
        {"mediaType":"application/vnd.ollama.image.messages","digest":format!("sha256:{digest}"),"size":10}
    ));
    std::fs::write(&mpath, m.to_string()).unwrap();
    dir
}

/// Ollama and llama-server both treat seed 0 as a real (deterministic) seed and -1 as random
/// (measured, see ARCHITECTURE.md): `PARAMETER seed` must be carried over as written.
#[test]
fn seed_parameter_is_kept_as_written() {
    let seed = |line: &str| {
        let r = import_text(
            &format!(
                "FROM ./x.gguf
PARAMETER {line}
"
            ),
            &ImportOptions::default(),
        )
        .unwrap();
        assert!(!has(&r, "invalid_value"), "{:?}", r.warnings);
        r.import.preset.unwrap().sampling.seed
    };
    assert_eq!(seed("seed 0"), Some(0));
    assert_eq!(seed("seed 42"), Some(42));
    assert_eq!(seed("seed -1"), Some(-1));
    assert_eq!(seed("seed 42.0"), Some(42));
    let r = import_text(
        "FROM ./x.gguf
PARAMETER temperature 0.5
",
        &ImportOptions::default(),
    )
    .unwrap();
    assert_eq!(r.import.preset.unwrap().sampling.seed, None);
}
