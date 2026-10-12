//! Domain store tests. Everything runs on temp directories; the migration tests start from
//! a copy of a real user's config directory (`tests/data/legacy-user-config`).

use super::*;
use crate::domain::launch::LaunchDefaults;
use crate::params::ParamValue;
use std::fs;

const FIXTURE: &str = "tests/data/legacy-user-config";
const QWEN4B: &str =
    r"D:\Models\unsloth\Qwen3-4B-Instruct-2507-GGUF\Qwen3-4B-Instruct-2507-Q4_K_M.gguf";
const GPTOSS: &str = r"D:\Models\ggml-org\gpt-oss-20b-GGUF\gpt-oss-20b-MXFP4.gguf";
const CODER: &str =
    r"D:\Models\Qwen\Qwen2.5-Coder-14B-Instruct-GGUF\qwen2.5-coder-14b-instruct-q4_k_m.gguf";

#[test]
fn persisted_definition_changes_publish_an_event() {
    // Hold the same lock as public writes so concurrent domain tests cannot publish here.
    let _guard = domain_lock();
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let mut state = domain.load_state().unwrap();
    let mut events = crate::events::subscribe();
    domain.commit(&mut state).unwrap();
    while let Ok(event) = events.try_recv() {
        assert!(!matches!(
            event,
            crate::events::CoreEvent::DefinitionsChanged
        ));
    }
    state.defs_dirty = true;
    domain.commit(&mut state).unwrap();
    let mut changed = false;
    while let Ok(event) = events.try_recv() {
        changed |= matches!(event, crate::events::CoreEvent::DefinitionsChanged);
    }
    assert!(changed);
    assert!(!state.defs_dirty);
    assert!(dir.path().join(DEFINITIONS_SCHEMA.file).is_file());
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE)
}

/// A temp config dir holding a copy of the real files.
fn real_copy() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(fixture_dir()).unwrap().flatten() {
        fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
    }
    dir
}

/// Defaults from the fixture's settings.json (what the app would use).
fn defaults() -> LaunchDefaults {
    let text = fs::read_to_string(fixture_dir().join("settings.json")).unwrap();
    let settings: crate::config::AppSettings = serde_json::from_str(&text).unwrap();
    LaunchDefaults::from_settings(&settings)
}

fn fixture_json(name: &str) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(fixture_dir().join(name)).unwrap()).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

fn legacy_config(model: &str) -> LaunchConfig {
    serde_json::from_value(fixture_json("last_session.json"))
        .map(|mut c: LaunchConfig| {
            c.model_path = model.into();
            c
        })
        .unwrap()
}

#[test]
fn real_user_config_migrates_to_v2_layout_without_changing_behavior() {
    let dir = real_copy();
    let domain = Domain::at(dir.path());
    let report = domain.migrate_legacy().unwrap();
    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!(
        (report.presets, report.profiles, report.session),
        (3, 0, true)
    );

    // Legacy files moved aside unchanged (they held no plaintext keys).
    for legacy in [LEGACY_PRESETS, LEGACY_SESSION] {
        assert!(!dir.path().join(legacy).exists());
        let backup = dir.path().join(format!("{}.pre-v2", legacy));
        assert_eq!(
            fs::read(&backup).unwrap(),
            fs::read(fixture_dir().join(legacy)).unwrap()
        );
    }

    // One definition per model, alias = file stem (what `--alias` already sent).
    let file = domain.definitions_file().unwrap();
    let mut aliases: Vec<_> = file.definitions.iter().map(|d| d.alias.as_str()).collect();
    aliases.sort();
    assert_eq!(
        aliases,
        [
            "Qwen3-4B-Instruct-2507-Q4_K_M",
            "gpt-oss-20b-MXFP4",
            "qwen2.5-coder-14b-instruct-q4_k_m"
        ]
    );
    assert_eq!(file.model_defaults.len(), 3);
    let gpt = &file.definitions[file
        .definitions
        .iter()
        .position(|d| d.alias == "gpt-oss-20b-MXFP4")
        .unwrap()];
    assert_eq!(gpt.extra_args.as_deref(), Some("--reasoning-effort high"));
    assert_eq!(gpt.load.get("context_size"), Some(&ParamValue::Int(131072)));
    assert_eq!(
        gpt.load.get("cache_type_k"),
        Some(&ParamValue::Text("q8_0".into()))
    );
    assert!(!gpt.load.contains_key("mcp_enabled"), "serve keys stay out");

    // The Dashboard gets exactly the old preset back for every model.
    let presets = fixture_json(LEGACY_PRESETS);
    for model in [QWEN4B, GPTOSS, CODER] {
        let original: ModelPreset = serde_json::from_value(presets[model].clone()).unwrap();
        let now = domain.model_preset(&defaults(), model).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&now).unwrap(),
            serde_json::to_value(&original).unwrap(),
            "{model}"
        );
    }

    // The last session is the "default" profile on gpt-oss's definition and resolves to the
    // same launch config.
    let session_profile = domain.profile(DEFAULT_SERVER_ID).unwrap();
    assert_eq!(
        session_profile.definition_id.as_deref(),
        Some(gpt.id.as_str())
    );
    let mut session = domain.session(&defaults()).unwrap().unwrap();
    assert_eq!(session.alias.take().as_deref(), Some("gpt-oss-20b-MXFP4"));
    assert_eq!(session.definition_id.take(), Some(gpt.id.clone()));
    // (The fixture was saved while `server_tools` existed; it still loads, the removed field
    // just is not written back.)
    let mut expected = fixture_json(LEGACY_SESSION);
    assert!(expected
        .as_object_mut()
        .unwrap()
        .remove("server_tools")
        .is_some());
    assert_eq!(serde_json::to_value(&session).unwrap(), expected);
    // ...and it is not one of the user's profiles.
    assert!(domain.profile_views(&defaults()).unwrap().is_empty());

    // New files carry schema_version.
    for schema in [DEFINITIONS_SCHEMA, PROFILES_SCHEMA] {
        let f = schema.file;
        let doc: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join(f)).unwrap()).unwrap();
        assert_eq!(doc["schema_version"], schema.current, "{f}");
    }

    // Running again is a no-op.
    let before = fs::read(dir.path().join(DEFINITIONS_SCHEMA.file)).unwrap();
    assert_eq!(domain.migrate_legacy().unwrap(), MigrationReport::default());
    assert_eq!(
        fs::read(dir.path().join(DEFINITIONS_SCHEMA.file)).unwrap(),
        before
    );
}

#[test]
fn real_user_files_of_other_services_get_versioned() {
    let dir = real_copy();
    let store = |f: &str| JsonStore::new(dir.path().join(f));

    // settings.json (unversioned = v1) -> v3 (network block, old default port 8080 to
    // automatic) with a backup; data kept.
    let settings = store("settings.json")
        .load_versioned::<crate::config::AppSettings>(&crate::config::SETTINGS_SCHEMA)
        .unwrap()
        .unwrap();
    assert_eq!((settings.version, settings.read_only), (3, false));
    assert_eq!(settings.data.server.port, 0);
    assert_eq!(settings.data.models_dir, r"D:\Models");
    assert_eq!(settings.data.server.host, "127.0.0.1");
    assert!(settings.data.network.cors_origins.is_empty());
    assert!(dir.path().join("settings.json.bak-v1").exists());

    // downloads.json and model_hashes.json were bare array / map (v1) -> wrapped v2 + backup.
    let hashes = store("model_hashes.json")
        .load_versioned::<std::collections::HashMap<String, String>>(
            &crate::downloader::HASHES_SCHEMA,
        )
        .unwrap()
        .unwrap();
    assert_eq!(hashes.version, 2);
    assert_eq!(hashes.data.len(), 3);
    let original: std::collections::HashMap<String, String> =
        serde_json::from_value(fixture_json("model_hashes.json")).unwrap();
    assert_eq!(hashes.data, original);
    assert_eq!(
        fs::read(dir.path().join("model_hashes.json.bak-v1")).unwrap(),
        fs::read(fixture_dir().join("model_hashes.json")).unwrap()
    );

    let jobs = store("downloads.json")
        .load_versioned::<serde_json::Value>(&crate::downloader::QUEUE_SCHEMA)
        .unwrap()
        .unwrap();
    assert_eq!(jobs.data, fixture_json("downloads.json"));
    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("downloads.json")).unwrap())
            .unwrap();
    assert_eq!(doc["schema_version"], 2);
    assert_eq!(doc["jobs"].as_array().unwrap().len(), 3);

    let status = data_files_status(dir.path());
    let get = |f: &str| status.iter().find(|s| s.file == f).unwrap().clone();
    assert_eq!(get("downloads.json").version, Some(2));
    assert_eq!(get("settings.json").version, Some(3));
    assert_eq!(get("server_profiles.json").version, None);
    assert!(status.iter().all(|s| !s.read_only));
}

/// Legacy profiles made from the real session config: one identical to the model's preset
/// (reuses its definition), one with other load settings (own definition), one router.
#[test]
fn legacy_profiles_reuse_or_split_definitions_and_keep_keys() {
    let dir = real_copy();
    let identical = legacy_config(GPTOSS); // the real session == gpt-oss preset
    let mut bigger = legacy_config(QWEN4B);
    bigger.context_size = 32768;
    bigger.port = 8091;
    bigger.mcp_enabled = true;
    let mut router = legacy_config(r"D:\Models");
    router.router_mode = true;
    router.port = 8092;
    let view = |id: &str, name: &str, config: &LaunchConfig, key: Option<&str>| {
        let mut v = serde_json::to_value(ProfileView {
            id: id.into(),
            name: name.into(),
            model_id: Some(config.model_path.to_string_lossy().to_string()),
            description: "OpenCode".into(),
            config: config.clone(),
            auto_start: id == "mig-agent",
            updated_at: 1790700000,
        })
        .unwrap();
        if let Some(k) = key {
            v["config"]["api_key"] = k.into(); // plaintext key of a pre-keychain build
        }
        v
    };
    let legacy = serde_json::json!([
        view("mig-gpt", "GPT OSS", &identical, None),
        view("mig-agent", "Ajan küçük", &bigger, Some("mig-plain-secret")),
        view("mig-router", "Router", &router, None),
    ]);
    fs::write(
        dir.path().join(LEGACY_PROFILES),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();

    let domain = Domain::at(dir.path());
    let report = domain.migrate_legacy().unwrap();
    assert_eq!(report.errors, Vec::<String>::new());
    assert_eq!(report.profiles, 3);

    let file = domain.definitions_file().unwrap();
    let default_of = |m: &str| file.model_defaults[m].definition_id.clone();
    let gpt = domain.profile("mig-gpt").unwrap();
    assert_eq!(
        gpt.definition_id,
        Some(default_of(GPTOSS)),
        "identical -> reused"
    );
    let agent = domain.profile("mig-agent").unwrap();
    assert_ne!(agent.definition_id, Some(default_of(QWEN4B)));
    let agent_def = file
        .definitions
        .iter()
        .find(|d| Some(&d.id) == agent.definition_id.as_ref())
        .unwrap();
    assert_eq!(agent_def.alias, "Qwen3-4B-Instruct-2507-Q4_K_M-Ajan-k-k");
    assert_eq!(
        agent.serve_overrides.get("mcp_enabled"),
        Some(&ParamValue::Bool(true))
    );
    assert!(agent.auto_start);
    assert_eq!(agent.updated_at, 1790700000);
    let r = domain.profile("mig-router").unwrap();
    assert!(r.definition_id.is_none() && r.router.is_some());

    // Plaintext key -> keychain under the old account; not in any file anymore.
    assert_eq!(agent.api_key_ref.as_deref(), Some("profile:mig-agent"));
    assert_eq!(
        crate::secrets::api_key_get("profile:mig-agent").as_deref(),
        Some("mig-plain-secret")
    );
    for n in names(dir.path()) {
        let text = fs::read_to_string(dir.path().join(&n)).unwrap_or_default();
        assert!(!text.contains("mig-plain-secret"), "{n}");
    }
    assert!(dir.path().join("profiles.json.pre-v2").exists());

    // The GUI sees the same profiles as before (config compared as JSON).
    let views = domain.profile_views(&defaults()).unwrap();
    assert_eq!(views.len(), 3);
    for (id, original) in [
        ("mig-gpt", &identical),
        ("mig-agent", &bigger),
        ("mig-router", &router),
    ] {
        let mut v = views.iter().find(|v| v.id == id).unwrap().clone();
        v.config.alias = None;
        v.config.definition_id = None;
        let mut expected = original.clone();
        expected.api_key = None;
        if id == "mig-agent" {
            expected.api_key_ref = Some("profile:mig-agent".into());
        }
        if id == "mig-router" {
            // A router's models directory is resolved like any configured directory: the
            // Windows path is absolute on Windows and data-root relative elsewhere.
            expected.model_path = crate::paths::resolve_dir(&expected.model_path.to_string_lossy());
        }
        assert_eq!(
            serde_json::to_value(&v.config).unwrap(),
            serde_json::to_value(&expected).unwrap(),
            "{id}"
        );
    }
    crate::secrets::api_key_delete("profile:mig-agent");
}

/// Keychain references of all three legacy owners keep resolving after the migration.
#[test]
fn keychain_refs_survive_migration() {
    let dir = real_copy();
    let preset_ref = crate::secrets::preset_ref(QWEN4B);
    crate::secrets::adopt_api_key(&preset_ref, Some("kc-preset"), None).unwrap();
    // Session and preset files referencing existing keychain entries...
    let mut presets = fixture_json(LEGACY_PRESETS);
    presets[QWEN4B]["api_key_ref"] = preset_ref.clone().into();
    // ...and a legacy plaintext session key.
    let mut session = fixture_json(LEGACY_SESSION);
    session["model_path"] = QWEN4B.into();
    session["api_key"] = "kc-session-plain".into();
    fs::write(dir.path().join(LEGACY_PRESETS), presets.to_string()).unwrap();
    fs::write(dir.path().join(LEGACY_SESSION), session.to_string()).unwrap();

    let domain = Domain::at(dir.path());
    domain.migrate_legacy().unwrap();
    let p = domain.model_preset(&defaults(), QWEN4B).unwrap().unwrap();
    assert_eq!(p.api_key_ref.as_deref(), Some(preset_ref.as_str()));
    assert_eq!(
        crate::secrets::api_key_get(&preset_ref).as_deref(),
        Some("kc-preset")
    );
    let s = domain.session(&defaults()).unwrap().unwrap();
    assert_eq!(s.api_key_ref.as_deref(), Some(crate::secrets::SESSION_REF));
    assert_eq!(
        crate::servers::resolve_api_key(&s).unwrap().as_deref(),
        Some("kc-session-plain")
    );
    let backup = fs::read_to_string(dir.path().join("last_session.json.pre-v2")).unwrap();
    assert!(!backup.contains("kc-session-plain"));
    assert!(
        backup.contains("gpu_layers"),
        "the rest of the backup is kept"
    );
    crate::secrets::api_key_delete(&preset_ref);
    crate::secrets::api_key_delete(crate::secrets::SESSION_REF);
}

#[test]
fn dashboard_changes_never_leak_into_profiles_sharing_a_definition() {
    let dir = real_copy();
    let domain = Domain::at(dir.path());
    domain.migrate_legacy().unwrap();
    let mut config = legacy_config(GPTOSS);
    config.port = 8095;
    domain
        .save_profile_view(&ProfileView {
            id: "cow".into(),
            name: "Paylaşan".into(),
            model_id: None,
            description: String::new(),
            config: config.clone(),
            auto_start: false,
            updated_at: 0,
        })
        .unwrap();
    let shared = domain.definitions_file().unwrap().model_defaults[GPTOSS]
        .definition_id
        .clone();
    assert_eq!(
        domain.profile("cow").unwrap().definition_id,
        Some(shared.clone())
    );

    // The Dashboard saves new settings for the model (as on every start).
    let mut preset = domain.model_preset(&defaults(), GPTOSS).unwrap().unwrap();
    preset.context_size = 8192;
    domain.save_model_preset(GPTOSS, &preset).unwrap();

    assert_eq!(
        domain
            .model_preset(&defaults(), GPTOSS)
            .unwrap()
            .unwrap()
            .context_size,
        8192
    );
    let file = domain.definitions_file().unwrap();
    assert_eq!(
        file.model_defaults[GPTOSS].definition_id, shared,
        "Dashboard keeps its id"
    );
    let cow = domain.profile("cow").unwrap();
    assert_ne!(cow.definition_id, Some(shared), "profile moved to a fork");
    let view = domain
        .profile_views(&defaults())
        .unwrap()
        .into_iter()
        .find(|v| v.id == "cow")
        .unwrap();
    assert_eq!(view.config.context_size, 131072, "profile unchanged");
    // The session (pinned by snapshot) is unchanged too.
    assert_eq!(
        domain.session(&defaults()).unwrap().unwrap().context_size,
        131072
    );

    // Editing the profile now updates its own (exclusive) definition in place.
    let fork = cow.definition_id.clone().unwrap();
    let mut edited = view.clone();
    edited.config.gpu_layers = 20;
    domain.save_profile_view(&edited).unwrap();
    let cow = domain.profile("cow").unwrap();
    assert_eq!(cow.definition_id, Some(fork.clone()));
    assert_eq!(
        domain.definition(&fork).unwrap().load.get("gpu_layers"),
        Some(&ParamValue::Int(20))
    );
    let count = domain.definitions().unwrap().len();
    domain.save_profile_view(&edited).unwrap();
    assert_eq!(
        domain.definitions().unwrap().len(),
        count,
        "saving again adds nothing"
    );
}

#[test]
fn definitions_crud_validates_alias_and_references() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let mut config = legacy_config(QWEN4B);
    config.context_size = 4096;
    domain
        .save_model_preset(QWEN4B, &ModelPreset::from_launch_config(&config))
        .unwrap();
    let base = domain.definitions().unwrap().remove(0);
    assert_eq!(base.alias, "Qwen3-4B-Instruct-2507-Q4_K_M");

    let mut new = base.clone();
    new.id = String::new();
    new.alias = "QWEN3-4B-INSTRUCT-2507-Q4_K_M".into(); // same alias, other case
    let e = domain.save_definition(new.clone()).unwrap_err();
    let (key, params) = crate::error_key::parse(&e).unwrap();
    assert_eq!(key, "definitions.alias_exists", "{e}");
    assert_eq!(params["other"], "Qwen3-4B-Instruct-2507-Q4_K_M");
    new.alias = "qwen coder".into();
    assert!(domain.save_definition(new.clone()).is_err());
    new.alias = "qwen-agent".into();
    new.load
        .insert("mmproj_path".into(), ParamValue::Text("mm.gguf".into()));
    new.default_preset_id = Some("builtin:coding".into());
    let saved = domain.save_definition(new).unwrap();
    assert!(saved.id.starts_with("md-") && saved.created_at > 0);
    assert_eq!(saved.mmproj, Some(PathBuf::from("mm.gguf")), "normalized");
    assert!(!saved.load.contains_key("mmproj_path"));

    let mut bad_preset = saved.clone();
    bad_preset.default_preset_id = Some("ip-missing".into());
    assert!(domain.save_definition(bad_preset).is_err());

    let copy = domain.duplicate_definition(&saved.id, None).unwrap();
    assert_eq!(copy.alias, "qwen-agent-copy");
    assert_ne!(copy.id, saved.id);
    assert_eq!(copy.load, saved.load);

    // A profile serving it blocks deletion.
    let mut pc = config.clone();
    pc.port = 8099;
    domain
        .save_profile_view(&ProfileView {
            id: "uses-copy".into(),
            name: "x".into(),
            model_id: None,
            description: String::new(),
            config: domain
                .resolve_definition(&defaults(), &copy.id, &ParamValues::new())
                .map(|mut c| {
                    c.port = 8099;
                    c
                })
                .unwrap(),
            auto_start: false,
            updated_at: 0,
        })
        .unwrap();
    // The form named its definition: the profile references exactly that one (not an
    // identical twin) and stores no overrides beyond the connection.
    let p = domain.profile("uses-copy").unwrap();
    assert_eq!(p.definition_id, Some(copy.id.clone()));
    assert!(p.serve_overrides.is_empty(), "{:?}", p.serve_overrides);
    let refs = domain.definition_refs(&copy.id).unwrap();
    assert_eq!(refs.profiles.len(), 1);
    assert_eq!(refs.profiles[0].id, "uses-copy");
    assert!(domain.delete_definition(&copy.id).is_err());
    domain.delete_definition(&saved.id).unwrap();
    domain.delete_profile("uses-copy").unwrap();
    // User-created definitions survive the profile's deletion.
    domain.delete_definition(&copy.id).unwrap();
    // Deleting the Dashboard default drops the model default entry.
    domain.delete_definition(&base.id).unwrap();
    assert!(domain.model_preset(&defaults(), QWEN4B).unwrap().is_none());
}

#[test]
fn inference_presets_crud_and_builtins_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let all = domain.inference_presets().unwrap();
    assert_eq!(all.len(), 5);
    let mut coding = domain.inference_preset("builtin:coding").unwrap();
    let e = domain.save_inference_preset(coding.clone()).unwrap_err();
    assert_eq!(e, "presets.builtin_read_only");
    let e = domain
        .delete_inference_preset("builtin:coding")
        .unwrap_err();
    assert_eq!(e, "presets.builtin_undeletable");

    // Copy + edit = the duplicate flow.
    coding.id = String::new();
    coding.builtin = false;
    coding.name = "Benim kod ayarım".into();
    coding.sampling.temperature = Some(0.1);
    coding.system_prompt = Some("Kısa cevap ver.".into());
    let saved = domain.save_inference_preset(coding).unwrap();
    assert!(saved.id.starts_with("ip-"));
    assert_eq!(domain.inference_presets().unwrap().len(), 6);
    let doc: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join(PRESETS_SCHEMA.file)).unwrap())
            .unwrap();
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["presets"][0]["sampling"]["temperature"], 0.1);

    // A definition defaulting to it falls back to none when it is deleted.
    let mut config = legacy_config(QWEN4B);
    config.port = 8080;
    domain
        .save_model_preset(QWEN4B, &ModelPreset::from_launch_config(&config))
        .unwrap();
    let mut d = domain.definitions().unwrap().remove(0);
    d.default_preset_id = Some(saved.id.clone());
    domain.save_definition(d.clone()).unwrap();
    domain.delete_inference_preset(&saved.id).unwrap();
    assert_eq!(domain.definition(&d.id).unwrap().default_preset_id, None);
    assert!(domain.delete_inference_preset(&saved.id).is_err());
}

#[test]
fn newer_domain_files_are_read_only_and_block_the_legacy_import() {
    let dir = real_copy();
    let newer =
        r#"{"schema_version": 7, "definitions": [], "model_defaults": {}, "from_the_future": 1}"#;
    fs::write(dir.path().join(DEFINITIONS_SCHEMA.file), newer).unwrap();
    let domain = Domain::at(dir.path());
    assert!(domain.definitions().unwrap().is_empty(), "still readable");
    // Nothing imported or moved: the legacy files stay for this (older) build.
    assert!(dir.path().join(LEGACY_PRESETS).exists());
    let mut d = ModelDefinition {
        id: String::new(),
        alias: "x".into(),
        base: ModelRef::Local {
            path: QWEN4B.into(),
        },
        description: String::new(),
        mmproj: None,
        adapters: vec![],
        draft: None,
        chat_template: None,
        chat_template_kwargs: None,
        extra_args: None,
        load: ParamValues::new(),
        default_preset_id: None,
        ttl_seconds: None,
        pinned: false,
        capture_content: false,
        content_retention_days: None,
        auto_created: false,
        created_at: 0,
        updated_at: 0,
    };
    let err = domain.save_definition(d.clone()).unwrap_err();
    assert!(err.starts_with("[store.newer_version]"), "{err}");
    d.alias = "y".into();
    assert!(domain
        .save_model_preset(QWEN4B, &ModelPreset::default())
        .is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join(DEFINITIONS_SCHEMA.file)).unwrap(),
        newer
    );
    let status = data_files_status(dir.path());
    assert!(status
        .iter()
        .any(|s| s.file == DEFINITIONS_SCHEMA.file && s.read_only && s.version == Some(7)));
}

#[test]
fn profile_resolution_layers_definition_overrides_and_transient() {
    let dir = real_copy();
    let domain = Domain::at(dir.path());
    domain.migrate_legacy().unwrap();
    let mut c = legacy_config(QWEN4B);
    c.gpu_layers = 37;
    c.context_size = 65536;
    c.cache_type_k = Some("q8_0".into());
    c.cache_type_v = Some("q8_0".into());
    c.parallel_slots = Some(1);
    c.custom_args = None;
    c.port = 8093;
    c.params.insert("timeout".into(), ParamValue::Int(900));
    domain
        .save_profile_view(&ProfileView {
            id: "layers".into(),
            name: "Katmanlar".into(),
            model_id: None,
            description: String::new(),
            config: c,
            auto_start: false,
            updated_at: 0,
        })
        .unwrap();
    let p = domain.profile("layers").unwrap();
    // Model-level settings equal the Qwen preset -> its Dashboard definition is reused; the
    // serve-only `timeout` sits in the profile.
    assert_eq!(
        p.definition_id,
        Some(
            domain.definitions_file().unwrap().model_defaults[QWEN4B]
                .definition_id
                .clone()
        )
    );
    assert_eq!(
        p.serve_overrides.get("timeout"),
        Some(&ParamValue::Int(900))
    );
    let mut transient = ParamValues::new();
    transient.insert("context_size".into(), ParamValue::Int(4096));
    let c = domain
        .resolve_profile(&defaults(), "layers", &transient)
        .unwrap();
    assert_eq!((c.gpu_layers, c.context_size, c.port), (37, 4096, 8093));
    assert_eq!(c.params.get("timeout"), Some(&ParamValue::Int(900)));
    assert!(domain
        .resolve_profile(&defaults(), "missing", &transient)
        .is_err());
}

/// A profile migrated from the real config starts a real llama-server through
/// ServerProfile -> ModelDefinition -> effective LaunchConfig. Run with
/// HS_LLAMA_SERVER=<llama-server> cargo test -p harness-core e2e_migrated_profile -- --ignored
#[tokio::test]
#[ignore]
async fn e2e_migrated_profile_starts_real_server() {
    let Ok(bin) = std::env::var("HS_LLAMA_SERVER") else {
        eprintln!("HS_LLAMA_SERVER not set, skipping");
        return;
    };
    if !Path::new(QWEN4B).is_file() {
        eprintln!("{QWEN4B} missing, skipping");
        return;
    }
    let dir = real_copy();
    let domain = Domain::at(dir.path());
    domain.migrate_legacy().unwrap();
    let mut preset = domain.model_preset(&defaults(), QWEN4B).unwrap().unwrap();
    let port = 18097;
    let mut config = preset.to_launch_config(Path::new(QWEN4B));
    config.host = "127.0.0.1".into();
    config.port = port;
    preset.api_key_ref = None;
    domain
        .save_profile_view(&ProfileView {
            id: "e2e-qwen".into(),
            name: "E2E".into(),
            model_id: None,
            description: String::new(),
            config,
            auto_start: false,
            updated_at: 0,
        })
        .unwrap();
    // Small context for the test run (transient layer on top of the migrated settings).
    let mut transient = ParamValues::new();
    transient.insert("context_size".into(), ParamValue::Int(2048));
    let mut config = domain
        .resolve_profile(&defaults(), "e2e-qwen", &transient)
        .unwrap();
    assert_eq!(config.gpu_layers, 37, "from the migrated definition");
    assert_eq!(config.cache_type_k.as_deref(), Some("q8_0"));
    config.binary_path = bin.into();

    let pm = crate::process::ProcessManager::with_id("e2e-qwen");
    pm.start(config).await.unwrap();
    let t = std::time::Instant::now();
    while pm.get_status() != crate::process::ServerStatus::Running {
        assert!(
            t.elapsed() < std::time::Duration::from_secs(90),
            "{:?}\n{}",
            pm.get_status(),
            pm.get_logs(Some(30)).join("\n")
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_err());
    pm.stop().await.unwrap();
    assert!(!pm.has_child());
    assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());
}

// --- Duplicate flow ---

fn profile_view(id: &str, name: &str, config: LaunchConfig) -> ProfileView {
    ProfileView {
        id: id.into(),
        name: name.into(),
        model_id: None,
        description: String::new(),
        config,
        auto_start: false,
        updated_at: 0,
    }
}

/// A definition saved from a form (user-created) for QWEN4B, with some load settings.
fn user_definition(domain: &Domain, alias: &str) -> ModelDefinition {
    let mut config = legacy_config(QWEN4B);
    config.definition_id = None;
    config.gpu_layers = 30;
    config.context_size = 8192;
    config.api_key_ref = None;
    domain.save_form(None, Some(alias), &config).unwrap()
}

#[test]
fn alias_rename_reaches_profiles_that_reference_the_definition() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let def = user_definition(&domain, "qwen-coder");
    let mut form = domain.definition_form(&defaults(), &def.id).unwrap();
    assert_eq!(form.alias.as_deref(), Some("qwen-coder"));
    form.port = 18190;
    domain
        .save_profile_view(&profile_view("coder", "Coder", form))
        .unwrap();
    assert_eq!(
        domain.profile("coder").unwrap().definition_id,
        Some(def.id.clone())
    );

    // Rename: the profile keeps the id and now serves the new alias.
    let mut renamed = domain.definition(&def.id).unwrap();
    renamed.alias = "qwen-agent".into();
    domain.save_definition(renamed).unwrap();
    let c = domain
        .resolve_profile(&defaults(), "coder", &ParamValues::new())
        .unwrap();
    assert_eq!(c.alias.as_deref(), Some("qwen-agent"));
    assert_eq!(c.definition_id.as_deref(), Some(def.id.as_str()));
    assert_eq!((c.gpu_layers, c.context_size, c.port), (30, 8192, 18190));
    let view = domain
        .profile_views(&defaults())
        .unwrap()
        .into_iter()
        .find(|v| v.id == "coder")
        .unwrap();
    assert_eq!(view.config.alias.as_deref(), Some("qwen-agent"));

    // Later edits of the definition reach the profile (it stores only differences).
    let mut edited = domain.definition(&def.id).unwrap();
    edited.load.insert("gpu_layers".into(), ParamValue::Int(12));
    domain.save_definition(edited).unwrap();
    let c = domain
        .resolve_profile(&defaults(), "coder", &ParamValues::new())
        .unwrap();
    assert_eq!(c.gpu_layers, 12);

    // An invalid or taken alias is refused.
    let mut bad = domain.definition(&def.id).unwrap();
    bad.alias = "qwen agent".into();
    assert!(domain.save_definition(bad).is_err());
    let other = user_definition(&domain, "other");
    let mut clash = domain.definition(&other.id).unwrap();
    clash.alias = "QWEN-AGENT".into();
    let e = domain.save_definition(clash).unwrap_err();
    assert_eq!(
        crate::error_key::parse(&e).map(|(k, _)| k),
        Some("definitions.alias_exists"),
        "{e}"
    );
}

#[test]
fn auto_created_definitions_are_cleaned_up_with_their_profile_only() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    // The Dashboard default for QWEN4B (auto: created by the compatibility layer).
    let mut dash = legacy_config(QWEN4B);
    dash.api_key_ref = None;
    domain
        .save_model_preset(QWEN4B, &ModelPreset::from_launch_config(&dash))
        .unwrap();
    let dash_def = domain.definitions().unwrap().remove(0);
    assert!(dash_def.auto_created);

    // Profiles without a named definition and other settings: each gets its own
    // auto-created `<stem>-<name>` definition.
    let split = |id: &str, name: &str, ctx: u32| {
        let mut c = dash.clone();
        c.context_size = ctx;
        c.port = 18191;
        domain
            .save_profile_view(&profile_view(id, name, c))
            .unwrap();
        domain.profile(id).unwrap().definition_id.unwrap()
    };
    let a = split("pa", "A", 1024);
    let b = split("pb", "B", 2048);
    assert!(domain.definition(&a).unwrap().auto_created);
    assert!(domain
        .definition(&a)
        .unwrap()
        .alias
        .starts_with("Qwen3-4B-Instruct-2507-Q4_K_M-A"));

    // Deleting the owner removes its orphaned auto definition.
    domain.delete_profile("pa").unwrap();
    assert!(domain.definition(&a).is_err());

    // A definition the user edited is theirs: kept.
    let mut adopted = domain.definition(&b).unwrap();
    adopted.description = "benim".into();
    let adopted = domain.save_definition(adopted).unwrap();
    assert!(!adopted.auto_created);
    domain.delete_profile("pb").unwrap();
    assert!(domain.definition(&b).is_ok());

    // Still referenced by another profile: kept until the last one goes.
    let shared = split("pc", "C", 4096);
    let mut form = domain
        .resolve_profile(&defaults(), "pc", &ParamValues::new())
        .unwrap();
    form.port = 18192;
    domain
        .save_profile_view(&profile_view("pd", "D", form))
        .unwrap();
    assert_eq!(
        domain.profile("pd").unwrap().definition_id,
        Some(shared.clone())
    );
    domain.delete_profile("pc").unwrap();
    assert!(domain.definition(&shared).is_ok(), "pd still uses it");
    domain.delete_profile("pd").unwrap();
    assert!(domain.definition(&shared).is_err());

    // The Dashboard default is never removed by profile cleanup, user definitions stay.
    let user = user_definition(&domain, "mine");
    let mut on_user = domain.definition_form(&defaults(), &user.id).unwrap();
    on_user.port = 18193;
    domain
        .save_profile_view(&profile_view("pe", "E", on_user))
        .unwrap();
    domain.delete_profile("pe").unwrap();
    assert!(domain.definition(&user.id).is_ok());
    assert!(domain.definition(&dash_def.id).is_ok());

    // A profile switching away from its auto definition releases it too.
    let f = split("pf", "F", 3072);
    let mut moved = domain.definition_form(&defaults(), &user.id).unwrap();
    moved.port = 18194;
    domain
        .save_profile_view(&profile_view("pf", "F", moved))
        .unwrap();
    assert_eq!(
        domain.profile("pf").unwrap().definition_id,
        Some(user.id.clone())
    );
    assert!(domain.definition(&f).is_err());
}

#[test]
fn form_start_layers_transient_changes_over_the_definition_without_saving() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let def = user_definition(&domain, "qwen-fast");
    let saved_file = fs::read(dir.path().join(DEFINITIONS_SCHEMA.file)).unwrap();

    let mut form = domain.definition_form(&defaults(), &def.id).unwrap();
    form.context_size = 2048; // changed
    form.threads = Some(4); // added
    form.flash_attn = !form.flash_attn;
    form.params.insert("timeout".into(), ParamValue::Int(30)); // serve-only
    form.alias = Some("forged".into()); // the alias always comes from the definition
    form.port = 18195;

    // The "unsaved changes" layer: model-level differences only.
    let transient = crate::domain::launch::transient_overrides(&defaults(), &def, &form).unwrap();
    let mut keys: Vec<_> = transient.keys().cloned().collect();
    keys.sort();
    assert_eq!(keys, ["context_size", "flash_attn", "threads"]);

    let c = domain.resolve_form(&defaults(), "default", &form).unwrap();
    assert_eq!(c.alias.as_deref(), Some("qwen-fast"));
    assert_eq!(c.definition_id.as_deref(), Some(def.id.as_str()));
    assert_eq!((c.context_size, c.threads, c.port), (2048, Some(4), 18195));
    assert_eq!(c.gpu_layers, 30, "from the definition");
    assert_eq!(c.params.get("timeout"), Some(&ParamValue::Int(30)));
    let mut expected = form.clone();
    expected.alias = Some("qwen-fast".into());
    assert_eq!(
        serde_json::to_value(&c).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    // Starting saved nothing.
    assert_eq!(
        fs::read(dir.path().join(DEFINITIONS_SCHEMA.file)).unwrap(),
        saved_file
    );

    // Over a profile that serves the definition: profile layer, then the form.
    let mut pform = domain.definition_form(&defaults(), &def.id).unwrap();
    pform.port = 18196;
    pform.parallel_slots = Some(2);
    domain
        .save_profile_view(&profile_view("fast", "Fast", pform.clone()))
        .unwrap();
    pform.context_size = 1024;
    let c = domain.resolve_form(&defaults(), "fast", &pform).unwrap();
    assert_eq!((c.context_size, c.parallel_slots), (1024, Some(2)));

    // A form whose file is not the definition's is refused; no definition = as is.
    let mut wrong = form.clone();
    wrong.model_path = GPTOSS.into();
    assert!(domain.resolve_form(&defaults(), "default", &wrong).is_err());
    let mut plain = form.clone();
    plain.definition_id = None;
    let c = domain.resolve_form(&defaults(), "default", &plain).unwrap();
    assert_eq!(c.alias, None, "file stem at launch");
}

#[test]
fn save_form_as_default_or_new_definition() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let def = user_definition(&domain, "qwen-a");
    assert_eq!(
        domain.dashboard_definition(QWEN4B).unwrap(),
        Some(def.id.clone())
    );
    let mut form = domain.definition_form(&defaults(), &def.id).unwrap();
    form.context_size = 16384;
    form.mcp_enabled = true;

    // Save as default: model part into the definition, serve part into the Dashboard entry.
    let saved = domain.save_form(Some(&def.id), None, &form).unwrap();
    assert_eq!(saved.alias, "qwen-a");
    assert_eq!(
        saved.load.get("context_size"),
        Some(&ParamValue::Int(16384))
    );
    assert!(!saved.load.contains_key("mcp_enabled"));
    let back = domain.definition_form(&defaults(), &def.id).unwrap();
    assert!(back.mcp_enabled);
    assert_eq!(back.context_size, 16384);

    // Save as new: another alias on the same file, becomes the Dashboard's definition.
    form.context_size = 4096;
    let new = domain.save_form(None, Some("qwen-b"), &form).unwrap();
    assert_ne!(new.id, def.id);
    assert_eq!(new.load.get("context_size"), Some(&ParamValue::Int(4096)));
    assert_eq!(domain.dashboard_definition(QWEN4B).unwrap(), Some(new.id));
    assert_eq!(
        domain.definition(&def.id).unwrap().load.get("context_size"),
        Some(&ParamValue::Int(16384)),
        "the source is unchanged"
    );
    assert!(domain.save_form(None, Some("QWEN-A"), &form).is_err());
    assert!(domain.save_form(None, Some("bad alias"), &form).is_err());
}

#[test]
fn several_adapters_survive_the_form() {
    use crate::domain::AdapterRef;
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let def = user_definition(&domain, "with-loras");
    let mut d = domain.definition(&def.id).unwrap();
    d.adapters = vec![
        AdapterRef {
            path: r"D:\Models\lora\a.gguf".into(),
            scale: Some(0.5),
        },
        AdapterRef {
            path: r"D:\Models\lora\b.gguf".into(),
            scale: None,
        },
    ];
    domain.save_definition(d).unwrap();
    // The form carries them as `lora_scaled`; saving the form keeps them as adapters.
    let form = domain.definition_form(&defaults(), &def.id).unwrap();
    assert!(form.params.contains_key("lora_scaled"));
    let saved = domain.save_form(Some(&def.id), None, &form).unwrap();
    assert_eq!(saved.adapters.len(), 2);
    assert_eq!(saved.adapters[0].scale, Some(0.5));
    assert_eq!(
        saved.adapters[1].path,
        PathBuf::from(r"D:\Models\lora\b.gguf")
    );
    assert!(!saved.load.contains_key("lora_scaled"));
}

#[test]
fn harness_toml_round_trip_with_relative_paths_and_hf_refs() {
    use crate::domain::{portable, AdapterRef};
    let models = tempfile::tempdir().unwrap();
    let rel = ["unsloth", "Qwen3-4B-GGUF", "Qwen3-4B-Q4_K_M.gguf"];
    let file = models.path().join(rel.iter().collect::<PathBuf>());
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"GGUF").unwrap();
    let mmproj = file.with_file_name("mmproj-F16.gguf");
    fs::write(&mmproj, b"GGUF").unwrap();
    let dirs = vec![models.path().to_path_buf()];

    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let mut preset = domain.inference_preset("builtin:coding").unwrap();
    preset.id = String::new();
    preset.builtin = false;
    preset.name = "Kod (kısa)".into();
    preset.system_prompt = Some("Kısa yaz.".into());
    preset.sampling.stop = vec!["</s>".into()];
    preset.sampling.logit_bias = Some(serde_json::json!({"15043": -1.5}));
    let preset = domain.save_inference_preset(preset).unwrap();
    let mut def = ModelDefinition::new("qwen-coder", ModelRef::Local { path: file.clone() });
    def.description = "Kod için".into();
    def.mmproj = Some(mmproj.clone());
    def.adapters = vec![
        AdapterRef {
            path: r"D:\elsewhere\lora.gguf".into(),
            scale: Some(0.7),
        },
        AdapterRef {
            path: r"D:\elsewhere\lora2.gguf".into(),
            scale: None,
        },
    ];
    def.chat_template_kwargs = Some(r#"{"enable_thinking":false}"#.into());
    def.load
        .insert("context_size".into(), ParamValue::Int(32768));
    def.load
        .insert("cache_type_k".into(), ParamValue::Text("q8_0".into()));
    def.load.insert("jinja".into(), ParamValue::Bool(true));
    def.load
        .insert("rope_freq_scale".into(), ParamValue::Float(0.5));
    def.load.insert(
        "override_kv".into(),
        ParamValue::List(vec!["a=int:1".into(), "b=str:x".into()]),
    );
    def.default_preset_id = Some(preset.id.clone());
    let def = domain.save_definition(def).unwrap();

    let text = domain.export_definition_toml(&def.id, true, &dirs).unwrap();
    assert!(
        text.contains("model = \"unsloth/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf\""),
        "{text}"
    );
    assert!(text.contains("source = \"hf:unsloth/Qwen3-4B-GGUF:Qwen3-4B-Q4_K_M.gguf\""));
    assert!(text.contains("mmproj = \"unsloth/Qwen3-4B-GGUF/mmproj-F16.gguf\""));
    assert!(
        text.contains("[preset]") || text.contains("[preset."),
        "{text}"
    );
    let models_text = models.path().to_string_lossy().replace('\\', "/");
    assert!(!text.replace("\\\\", "/").contains(&models_text), "{text}");
    let exported = dir.path().join(portable::file_name(&def.alias));
    fs::write(&exported, &text).unwrap(); // a real file on disk

    // Imported on a machine whose models live elsewhere: relative paths resolve there.
    let other = tempfile::tempdir().unwrap();
    let moved = other.path().join(rel.iter().collect::<PathBuf>());
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::copy(&file, &moved).unwrap();
    fs::copy(&mmproj, moved.with_file_name("mmproj-F16.gguf")).unwrap();
    let text = fs::read_to_string(&exported).unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let domain2 = Domain::at(dir2.path());
    let imp = domain2
        .preview_definition_import(&text, &[other.path().to_path_buf()])
        .unwrap();
    assert!(imp.model_found);
    assert_eq!(imp.definition.base.local_path(), Some(moved.as_path()));
    assert_eq!(
        imp.definition.mmproj.as_deref(),
        Some(moved.with_file_name("mmproj-F16.gguf").as_path())
    );
    assert_eq!(
        imp.missing,
        vec![
            r"D:\elsewhere\lora.gguf".to_string(),
            r"D:\elsewhere\lora2.gguf".to_string()
        ]
    );
    assert_eq!(imp.definition.load, def.load);
    assert_eq!(imp.definition.adapters.len(), 2);
    assert_eq!(imp.definition.adapters[0].scale, Some(0.7));
    assert_eq!(imp.definition.adapters[1].scale, None);
    assert_eq!(imp.definition.description, "Kod için");
    assert_eq!(
        imp.definition.chat_template_kwargs,
        def.chat_template_kwargs
    );
    assert!(!imp.alias_taken);
    let p = imp.preset.clone().unwrap();
    assert_eq!(p.name, "Kod (kısa)");
    assert_eq!(p.sampling, preset.sampling);
    assert_eq!(p.system_prompt, preset.system_prompt);
    let saved = domain2.import_definition(imp, "qwen-coder").unwrap();
    let linked = domain2
        .inference_preset(saved.default_preset_id.as_deref().unwrap())
        .unwrap();
    assert_eq!(linked.sampling, preset.sampling);

    // Second import: alias taken -> `-2` suggested.
    let again = domain2
        .preview_definition_import(&text, &[other.path().to_path_buf()])
        .unwrap();
    assert!(again.alias_taken);
    assert_eq!(again.suggested_alias, "qwen-coder-2");
    assert!(domain2
        .import_definition(again.clone(), "qwen-coder")
        .is_err());
    domain2.import_definition(again, "qwen-coder-2").unwrap();

    // Model not on this machine: the Hugging Face origin is offered.
    let empty = tempfile::tempdir().unwrap();
    let missing = domain2
        .preview_definition_import(&text, &[empty.path().to_path_buf()])
        .unwrap();
    assert!(!missing.model_found);
    assert_eq!(
        missing.hf_ref.as_deref(),
        Some("hf:unsloth/Qwen3-4B-GGUF:Qwen3-4B-Q4_K_M.gguf")
    );
    assert_eq!(
        missing.definition.base,
        ModelRef::parse("hf:unsloth/Qwen3-4B-GGUF:Qwen3-4B-Q4_K_M.gguf")
    );

    // A definition of a model not downloaded yet exports its hf ref, and a built-in
    // default preset is referenced by id (not embedded).
    let mut hf = ModelDefinition::new(
        "later",
        ModelRef::parse("hf:ggml-org/gpt-oss-20b-GGUF:gpt-oss-20b-MXFP4.gguf"),
    );
    hf.default_preset_id = Some("builtin:reasoning".into());
    let hf = domain.save_definition(hf).unwrap();
    let text = domain.export_definition_toml(&hf.id, true, &dirs).unwrap();
    assert!(text.contains("model = \"hf:ggml-org/gpt-oss-20b-GGUF:gpt-oss-20b-MXFP4.gguf\""));
    assert!(text.contains("default_preset = \"builtin:reasoning\""));
    assert!(!text.contains("[preset"));
    let imp = portable::parse_definition(&text, &dirs, &[]).unwrap();
    assert_eq!(
        imp.definition.default_preset_id.as_deref(),
        Some("builtin:reasoning")
    );
    assert!(!imp.model_found);

    // Wrong format / invalid alias are errors.
    assert!(
        portable::parse_definition("format = \"x\"\nalias = \"a\"\nmodel = \"m\"", &dirs, &[])
            .is_err()
    );
    let bad_alias = format!(
        "format = \"{}\"\nalias = \"a b\"\nmodel = \"m\"",
        portable::FORMAT
    );
    assert!(portable::parse_definition(&bad_alias, &dirs, &[]).is_err());
}

#[test]
fn harness_toml_embeds_the_chat_template_file_text() {
    let models = tempfile::tempdir().unwrap();
    let file = models.path().join("m.gguf");
    fs::write(&file, b"GGUF").unwrap();
    let dirs = vec![models.path().to_path_buf()];

    let src_dir = tempfile::tempdir().unwrap();
    let src = Domain::at(src_dir.path());
    let template = "{% for m in messages %}<|{{ m.role }}|>{{ m.content }}\n{% endfor %}";
    let saved =
        crate::chat_template::save_template(&src.chat_templates_dir(), "özel şablon", template)
            .unwrap();
    let mut def = ModelDefinition::new("tmpl-model", ModelRef::Local { path: file });
    def.load.insert(
        "chat_template_file".into(),
        ParamValue::Text(saved.path.to_string_lossy().to_string()),
    );
    def.load.insert("jinja".into(), ParamValue::Bool(true));
    let def = src.save_definition(def).unwrap();

    // The text travels, the machine-specific path does not.
    let text = src.export_definition_toml(&def.id, false, &dirs).unwrap();
    assert!(text.contains("chat_template_text"), "{text}");
    assert!(!text.contains("chat_template_file"), "{text}");
    assert!(!text.contains("özel şablon"), "{text}");

    // Import elsewhere: saved as a template file the definition points at.
    let dst_dir = tempfile::tempdir().unwrap();
    let dst = Domain::at(dst_dir.path());
    let imp = dst.preview_definition_import(&text, &dirs).unwrap();
    assert_eq!(imp.chat_template_text.as_deref(), Some(template));
    assert!(!imp.definition.load.contains_key("chat_template_file"));
    let saved = dst.import_definition(imp, "tmpl-model").unwrap();
    let Some(ParamValue::Text(path)) = saved.load.get("chat_template_file") else {
        panic!("chat_template_file missing: {:?}", saved.load);
    };
    assert!(
        std::path::Path::new(path).starts_with(dst.chat_templates_dir()),
        "{path}"
    );
    assert_eq!(fs::read_to_string(path).unwrap(), template);
    assert_eq!(saved.load.get("jinja"), Some(&ParamValue::Bool(true)));

    // A file that cannot be read keeps its path instead of silently dropping the setting.
    let mut gone = ModelDefinition::new(
        "gone",
        ModelRef::Local {
            path: models.path().join("m.gguf"),
        },
    );
    gone.load.insert(
        "chat_template_file".into(),
        ParamValue::Text(
            src_dir
                .path()
                .join("nope.jinja")
                .to_string_lossy()
                .to_string(),
        ),
    );
    let gone = src.save_definition(gone).unwrap();
    let text = src.export_definition_toml(&gone.id, false, &dirs).unwrap();
    assert!(text.contains("chat_template_file"), "{text}");
    assert!(!text.contains("chat_template_text"), "{text}");
}

#[test]
fn preset_export_import_round_trip_and_validation() {
    use crate::domain::presets::{export_presets, parse_presets, PRESET_FORMAT};
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let mut p = domain.inference_preset("builtin:creative").unwrap();
    p.id = String::new();
    p.builtin = false;
    p.name = "Hikâye".into();
    p.system_prompt = Some("Anlatıcı ol.".into());
    let s = &mut p.sampling;
    s.typical_p = Some(0.9);
    s.top_n_sigma = Some(1.5);
    s.repeat_last_n = Some(64);
    s.presence_penalty = Some(0.3);
    s.frequency_penalty = Some(0.2);
    s.dry_base = Some(1.75);
    s.dry_allowed_length = Some(2);
    s.dry_penalty_last_n = Some(-1);
    s.dry_sequence_breakers = vec!["\n".into(), ":".into()];
    s.mirostat = Some(2);
    s.mirostat_tau = Some(5.0);
    s.mirostat_eta = Some(0.1);
    s.seed = Some(7);
    s.max_tokens = Some(512);
    s.stop = vec!["###".into()];
    s.samplers = vec!["top_k".into(), "min_p".into(), "temperature".into()];
    s.logit_bias = Some(serde_json::json!([[15043, -2.0]]));
    s.grammar = Some("root ::= \"a\"".into());
    p.reasoning.effort = Some("low".into());
    p.reasoning.budget = Some(256);
    p.tool_choice = Some(serde_json::json!("required"));
    p.response_format = Some(serde_json::json!({"type": "json_object"}));
    let a = domain.save_inference_preset(p).unwrap();
    let mut b = domain.inference_preset("builtin:deterministic").unwrap();
    b.id = String::new();
    b.builtin = false;
    b.name = "Sabit".into();
    b.sampling.json_schema = Some(serde_json::json!({"type": "object"}));
    let b = domain.save_inference_preset(b).unwrap();

    let text = domain.export_inference_presets(None).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["format"], PRESET_FORMAT);
    assert_eq!(doc["presets"].as_array().unwrap().len(), 2);
    let parsed = parse_presets(&text).unwrap();
    for (got, want) in parsed.iter().zip([&a, &b]) {
        assert!(got.id.is_empty() && !got.builtin);
        assert_eq!(got.name, want.name);
        assert_eq!(got.sampling, want.sampling);
        assert_eq!(got.reasoning, want.reasoning);
        assert_eq!(got.system_prompt, want.system_prompt);
        assert_eq!(got.tool_choice, want.tool_choice);
        assert_eq!(got.response_format, want.response_format);
    }

    // The request body uses llama-server's names.
    let body = serde_json::Value::Object(a.request_params());
    assert_eq!(body["dry_sequence_breakers"][1], ":");
    assert_eq!(body["samplers"][0], "top_k");
    assert_eq!(body["reasoning_effort"], "low");
    assert_eq!(body["thinking_budget_tokens"], 256);
    assert_eq!(body["tool_choice"], "required");

    // Importing adds copies with new ids; taken names get a suffix.
    let imported = domain.import_inference_presets(&text).unwrap();
    assert_eq!(imported.len(), 2);
    assert_eq!(imported[0].name, "Hikâye (2)");
    assert_ne!(imported[0].id, a.id);
    assert_eq!(domain.inference_presets().unwrap().len(), 5 + 4);
    // Exporting a built-in gives an editable copy.
    let text = domain
        .export_inference_presets(Some(&["builtin:coding".to_string()]))
        .unwrap();
    let copy = domain.import_inference_presets(&text).unwrap().remove(0);
    assert!(!copy.builtin && !copy.id.starts_with("builtin:"));
    assert_eq!(copy.name, "Coding (2)");
    // A single bare preset object is accepted too.
    let one = serde_json::to_string(&b).unwrap();
    assert_eq!(parse_presets(&one).unwrap().len(), 1);

    // Invalid content: nothing is saved.
    let before = domain.inference_presets().unwrap().len();
    let bad = |f: &dyn Fn(&mut serde_json::Value)| {
        let mut v: serde_json::Value =
            serde_json::from_str(&export_presets(&[a.clone(), b.clone()])).unwrap();
        f(&mut v);
        v.to_string()
    };
    let cases = [
        ("format", bad(&|v| v["format"] = "harness.preset.v9".into())),
        (
            "top_p",
            bad(&|v| v["presets"][1]["sampling"]["top_p"] = 3.0.into()),
        ),
        (
            "sampler",
            bad(&|v| v["presets"][0]["sampling"]["samplers"] = serde_json::json!(["nope"])),
        ),
        (
            "tool_choice",
            bad(&|v| v["presets"][0]["tool_choice"] = "sometimes".into()),
        ),
        (
            "grammar+schema",
            bad(&|v| v["presets"][1]["sampling"]["grammar"] = "root ::= \"x\"".into()),
        ),
        (
            "reasoning",
            bad(&|v| v["presets"][0]["reasoning"]["format"] = "fancy".into()),
        ),
        (
            "logit_bias",
            bad(&|v| v["presets"][0]["sampling"]["logit_bias"] = 3.into()),
        ),
        (
            "empty",
            r#"{"format":"harness.preset.v1","presets":[]}"#.to_string(),
        ),
        ("json", "{nope".to_string()),
    ];
    for (case, text) in cases {
        assert!(domain.import_inference_presets(&text).is_err(), "{case}");
    }
    assert_eq!(domain.inference_presets().unwrap().len(), before);
}

/// A definition with an alias, started through the orchestrator from a Dashboard-style form
/// with an unsaved change, serves the alias as its model id (`/v1/models`). Run with
/// HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-core
///   e2e_definition_alias -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn e2e_definition_alias_is_the_served_model_id() {
    let (Ok(bin), Ok(model)) = (
        std::env::var("HS_LLAMA_SERVER"),
        std::env::var("HS_TEST_MODEL"),
    ) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_MODEL not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let registry = crate::servers::ServerRegistry::default();
    let port = registry.suggest_port("127.0.0.1", 18197, &[]).unwrap();
    let mut form = LaunchConfig {
        model_path: model.into(),
        gpu_layers: 99,
        context_size: 2048,
        host: "127.0.0.1".into(),
        port,
        ..Default::default()
    };
    let def = domain
        .save_form(None, Some("e2e-qwen-alias"), &form)
        .unwrap();
    // Start from the form with an unsaved change (transient layer).
    form.definition_id = Some(def.id.clone());
    form.context_size = 1024;
    let config = domain
        .resolve_form(&LaunchDefaults::from_config(&form), "e2e-alias", &form)
        .unwrap();
    assert_eq!(config.alias.as_deref(), Some("e2e-qwen-alias"));
    assert_eq!(config.context_size, 1024);
    assert_eq!(
        domain.definition(&def.id).unwrap().load.get("context_size"),
        Some(&ParamValue::Int(2048)),
        "starting saved nothing"
    );

    let gate = crate::operation_gate::OperationGate::default();
    // Default settings (loopback bind): nothing of the user's configuration is read.
    let settings = crate::config::AppSettings::default();
    crate::orchestrator::launch_with_binary(
        &registry,
        &gate,
        "e2e-alias".into(),
        config,
        bin.into(),
        &settings,
    )
    .await
    .unwrap();
    let pm = registry.get("e2e-alias").unwrap();
    let t = std::time::Instant::now();
    while pm.get_status() != crate::process::ServerStatus::Running {
        if t.elapsed() > std::time::Duration::from_secs(120) {
            let logs = pm.get_logs(Some(30)).join("\n");
            let _ = pm.stop().await;
            panic!("{:?}\n{}", pm.get_status(), logs);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let body: Result<serde_json::Value, String> = async {
        reqwest::get(format!("http://127.0.0.1:{}/v1/models", port))
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    // The Playground picker sees it running under the definition.
    let states = crate::domain::status::definition_states(
        std::slice::from_ref(&def),
        &[("e2e-alias".into(), pm.get_status(), pm.launch_config())],
    );
    pm.stop().await.unwrap();
    assert!(!pm.has_child());
    assert!(crate::servers::ensure_port_free("127.0.0.1", port).is_ok());

    assert_eq!(states[0].state, crate::domain::status::RunState::Running);
    let body = body.unwrap();
    let ids: Vec<&str> = body["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    eprintln!("/v1/models ids: {ids:?}");
    assert_eq!(ids, ["e2e-qwen-alias"]);
}

#[test]
fn profile_alias_drops_port_and_repeated_stem() {
    let stem = "nomic-embed-text-v1.5.Q8_0";
    // The auto name of a new server: `<stem> :<port>`.
    assert_eq!(
        profile_alias_base(stem, "nomic-embed-text-v1.5.Q8_0 :8082"),
        stem
    );
    assert_eq!(
        profile_alias_base(stem, "nomic-embed-text-v1.5.Q8_0-:8082"),
        stem
    );
    // A name that adds something keeps it after the stem.
    assert_eq!(
        profile_alias_base("qwen3", "Kod asistanı"),
        "qwen3-Kod-asistan"
    );
    assert_eq!(profile_alias_base("qwen3", "hızlı"), "qwen3-h-zl");
    assert_eq!(profile_alias_base("qwen3", "work"), "qwen3-work");
    // Router names and empty names fall back to the stem.
    assert_eq!(profile_alias_base("qwen3", " :8081"), "qwen3");
    // No colon or port ever ends up in the served id.
    assert!(!profile_alias_base("qwen3", "prod :8080 x").contains(':'));
}

#[test]
fn repaired_auto_alias_matches_only_the_old_generated_shape() {
    let stem = "nomic-embed-text-v1.5.Q8_0";
    let broken = "nomic-embed-text-v1.5.Q8_0-nomic-embed-text-v1.5.Q8_0-:8082";
    assert_eq!(repaired_auto_alias(stem, broken).as_deref(), Some(stem));
    // With the `-<n>` suffix `unique_alias` added when it was taken.
    assert_eq!(
        repaired_auto_alias(stem, &format!("{broken}-2")).as_deref(),
        Some(stem)
    );
    // Auto name of a linked Ollama model (`llama3.1:8b :8081`).
    assert_eq!(
        repaired_auto_alias("llama", "llama-llama3.1:8b-:8081").as_deref(),
        Some("llama3.1-8b")
    );
    // Anything else is left alone: user aliases, other stems, no port, clean aliases.
    for alias in [
        stem,
        "nomic-embed-text-v1.5.Q8_0-work",
        "nomic-embed-text-v1.5.Q8_0-nomic-embed-text-v1.5.Q8_0",
        "nomic-embed-text-v1.5.Q8_0-:8082",
        "nomic-embed-text-v1.5.Q8_0-x-:80a",
        "nomic-embed-text-v1.5.Q8_0-x-:8082-b",
        "my:model",
        "other-nomic-embed-text-v1.5.Q8_0-:8082",
        "NOMIC-EMBED-TEXT-V1.5.Q8_0-nomic-embed-text-v1.5.Q8_0-:8082",
    ] {
        assert_eq!(repaired_auto_alias(stem, alias), None, "{alias}");
    }
}

/// `model_definitions.json` v1 -> v2 rewrites the broken auto aliases (`<stem>-<stem>-:<port>`)
/// to the clean base, keeps every other alias, stays unique and leaves a `.bak-v1` backup.
#[test]
fn definitions_v2_migration_repairs_legacy_server_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let nomic = r"D:\Models\nomic-ai\nomic-embed-text-v1.5-GGUF\nomic-embed-text-v1.5.Q8_0.gguf";
    let def = |id: &str, alias: &str, path: &str, auto: bool| {
        serde_json::json!({
            "id": id, "alias": alias,
            "base": {"kind": "local", "path": path},
            "load": {"context_size": 2048},
            "auto_created": auto, "created_at": 1, "updated_at": 1,
        })
    };
    let v1 = serde_json::json!({
        "schema_version": 1,
        "definitions": [
            // The Dashboard definition already holds the clean name.
            def("md-dash", "nomic-embed-text-v1.5.Q8_0", nomic, true),
            def("md-srv", "nomic-embed-text-v1.5.Q8_0-nomic-embed-text-v1.5.Q8_0-:8082", nomic, true),
            def("md-qwen", "Qwen3-4B-Instruct-2507-Q4_K_M-Qwen3-4B-Instruct-2507-Q4_K_M-:8081-2", QWEN4B, false),
            def("md-user", "Qwen3-4B-Instruct-2507-Q4_K_M-work", QWEN4B, false),
            def("md-colon", "my:model", QWEN4B, false),
            {
                "id": "md-hf", "alias": "x-x-:8080",
                "base": {"kind": "hf", "repo": "o/r", "file": "x.gguf"},
                "created_at": 1, "updated_at": 1,
            },
        ],
        "model_defaults": {
            nomic: {"definition_id": "md-dash"},
        },
    });
    let path = dir.path().join(DEFINITIONS_SCHEMA.file);
    let original = serde_json::to_vec_pretty(&v1).unwrap();
    fs::write(&path, &original).unwrap();

    let domain = Domain::at(dir.path());
    let defs = domain.definitions().unwrap();
    let alias = |id: &str| defs.iter().find(|d| d.id == id).unwrap().alias.clone();
    assert_eq!(alias("md-dash"), "nomic-embed-text-v1.5.Q8_0");
    assert_eq!(alias("md-srv"), "nomic-embed-text-v1.5.Q8_0-2");
    assert_eq!(alias("md-qwen"), "Qwen3-4B-Instruct-2507-Q4_K_M");
    assert_eq!(alias("md-user"), "Qwen3-4B-Instruct-2507-Q4_K_M-work");
    assert_eq!(alias("md-colon"), "my:model");
    assert_eq!(alias("md-hf"), "x-x-:8080");
    // Everything else of the definitions is kept.
    let srv = defs.iter().find(|d| d.id == "md-srv").unwrap();
    assert!(srv.auto_created);
    assert_eq!(srv.load.get("context_size"), Some(&ParamValue::Int(2048)));
    assert_eq!(
        domain.definitions_file().unwrap().model_defaults[nomic].definition_id,
        "md-dash"
    );

    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["schema_version"], 3);
    assert_eq!(
        fs::read(dir.path().join("model_definitions.json.bak-v1")).unwrap(),
        original
    );
    // Once migrated nothing changes again.
    let after = fs::read(&path).unwrap();
    domain.definitions().unwrap();
    assert_eq!(fs::read(&path).unwrap(), after);
}

/// Two servers sharing one definition ("+ New server" from a selected server) never serve the
/// same model id: the second one gets `<alias>-<port>`, and both still count as serving the
/// definition (the gateway routes the definition alias to either). Run with
/// HS_LLAMA_SERVER=<llama-server> HS_TEST_EMBED_MODEL=<gguf> (or HS_TEST_MODEL) cargo test
///   -p harness-core e2e_shared_definition -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn e2e_shared_definition_servers_get_distinct_served_ids() {
    let model = std::env::var("HS_TEST_EMBED_MODEL").or_else(|_| std::env::var("HS_TEST_MODEL"));
    let (Ok(bin), Ok(model)) = (std::env::var("HS_LLAMA_SERVER"), model) else {
        eprintln!("HS_LLAMA_SERVER / HS_TEST_EMBED_MODEL not set, skipping");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    let registry = crate::servers::ServerRegistry::default();
    let gate = crate::operation_gate::OperationGate::default();
    let settings = crate::config::AppSettings::default();
    let form = LaunchConfig {
        model_path: model.into(),
        gpu_layers: 0,
        context_size: 512,
        host: "127.0.0.1".into(),
        port: 19400,
        ..Default::default()
    };
    let def = domain.save_form(None, Some("e2e-shared"), &form).unwrap();
    let mut ports = Vec::new();
    for (i, id) in ["e2e-shared-a", "e2e-shared-b"].into_iter().enumerate() {
        let port = registry
            .suggest_port("127.0.0.1", 19400 + i as u16, &ports)
            .unwrap();
        ports.push(port);
        let mut config = domain
            .resolve_definition(
                &LaunchDefaults::from_config(&form),
                &def.id,
                &ParamValues::new(),
            )
            .unwrap();
        config.port = port;
        crate::orchestrator::launch_with_binary(
            &registry,
            &gate,
            id.into(),
            config,
            bin.clone().into(),
            &settings,
        )
        .await
        .unwrap();
    }
    let mut served = Vec::new();
    let mut failure = None;
    for (id, port) in ["e2e-shared-a", "e2e-shared-b"].iter().zip(&ports) {
        let pm = registry.get(id).unwrap();
        let t = std::time::Instant::now();
        while pm.get_status() != crate::process::ServerStatus::Running {
            if t.elapsed() > std::time::Duration::from_secs(120) {
                failure = Some(format!(
                    "{:?}\n{}",
                    pm.get_status(),
                    pm.get_logs(Some(30)).join("\n")
                ));
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let body: Option<serde_json::Value> = async {
            reqwest::get(format!("http://127.0.0.1:{}/v1/models", port))
                .await
                .ok()?
                .json()
                .await
                .ok()
        }
        .await;
        served.push(body.and_then(|b| b["data"][0]["id"].as_str().map(str::to_string)));
    }
    let views: Vec<crate::domain::status::ServerView> = ["e2e-shared-a", "e2e-shared-b"]
        .iter()
        .map(|id| {
            let pm = registry.get(id).unwrap();
            (id.to_string(), pm.get_status(), pm.launch_config())
        })
        .collect();
    let states = crate::domain::status::definition_states(std::slice::from_ref(&def), &views);
    for id in ["e2e-shared-a", "e2e-shared-b"] {
        registry.get(id).unwrap().stop().await.unwrap();
    }
    for port in &ports {
        assert!(crate::servers::ensure_port_free("127.0.0.1", *port).is_ok());
    }
    if let Some(f) = failure {
        panic!("{f}");
    }
    eprintln!("served ids: {served:?}");
    assert_eq!(served[0].as_deref(), Some("e2e-shared"));
    assert_eq!(
        served[1].as_deref(),
        Some(format!("e2e-shared-{}", ports[1]).as_str())
    );
    assert_eq!(states[0].servers.len(), 2, "both serve the definition");
}

/// The Dashboard server has one source of truth: its saved `"default"` profile. The settings
/// defaults only describe a Dashboard server that was never saved, so after a restart the
/// form, the Servers list and the title bar all see the saved model, settings and port even
/// when the settings' default port says something else.
#[test]
fn default_server_is_the_saved_session_not_the_settings_defaults() {
    let dir = real_copy();
    // Settings changed after the last start: another default port and context.
    let settings = LaunchDefaults {
        port: 8888,
        context_size: 2048,
        ..defaults()
    };

    // Never saved: the settings defaults, no model.
    let empty = tempfile::tempdir().unwrap();
    let fresh = Domain::at(empty.path()).default_server(&settings).unwrap();
    assert!(!fresh.saved);
    assert_eq!((fresh.config.port, fresh.config.context_size), (8888, 2048));
    assert!(fresh.config.model_path.as_os_str().is_empty());
    assert_eq!(fresh.config.definition_id, None);

    // The real user's last session (imported from last_session.json) wins over the settings.
    let domain = Domain::at(dir.path());
    let restored = domain.default_server(&settings).unwrap();
    assert!(restored.saved);
    assert_eq!(restored.config.port, 8080);
    assert_eq!(restored.config.model_path, PathBuf::from(GPTOSS));
    assert_eq!(restored.config.context_size, 131072);
    assert_eq!(
        serde_json::to_value(&restored.config).unwrap(),
        serde_json::to_value(domain.session(&settings).unwrap().unwrap()).unwrap(),
        "the view and the restore path read the same launch"
    );

    // A Dashboard start on another model/port replaces it; a new process (app restart) reads
    // back exactly what was started.
    let form = LaunchConfig {
        port: 8090,
        context_size: 32768,
        gpu_layers: 33,
        ..legacy_config(QWEN4B)
    };
    domain.save_session(&form).unwrap();
    let after_restart = Domain::at(dir.path()).default_server(&settings).unwrap();
    assert!(after_restart.saved);
    assert_eq!(after_restart.config.model_path, PathBuf::from(QWEN4B));
    assert_eq!(
        (
            after_restart.config.port,
            after_restart.config.context_size,
            after_restart.config.gpu_layers
        ),
        (8090, 32768, 33)
    );
    let def_id = after_restart.config.definition_id.clone().unwrap();
    assert_eq!(
        domain.definition(&def_id).unwrap().alias,
        "Qwen3-4B-Instruct-2507-Q4_K_M"
    );
    // Still not one of the user's editable profiles.
    assert!(domain
        .profile_views(&settings)
        .unwrap()
        .iter()
        .all(|p| p.id != DEFAULT_SERVER_ID));

    // Its definition deleted: the launch no longer resolves, but the saved address stays
    // (the port shown never jumps back to the settings default).
    domain.delete_definition(&def_id).unwrap();
    let orphan = domain.default_server(&settings).unwrap();
    assert!(!orphan.saved);
    assert_eq!(orphan.config.port, 8090);
    assert_eq!(orphan.config.context_size, 2048);
    assert!(
        domain.profile(DEFAULT_SERVER_ID).is_ok(),
        "nothing is deleted"
    );
}

#[test]
fn first_dashboard_save_reuses_the_definition_the_download_made() {
    let dir = tempfile::tempdir().unwrap();
    let domain = Domain::at(dir.path());
    // A finished download gives the file a bare definition (Model Hub, `ensureDefinition`).
    let made = domain
        .save_definition(ModelDefinition::new(
            "qwen3-4b",
            ModelRef::Local {
                path: PathBuf::from(QWEN4B),
            },
        ))
        .unwrap();
    assert_eq!(
        domain.dashboard_definition(QWEN4B).unwrap(),
        Some(made.id.clone())
    );
    // Starting it from the Dashboard saves the form: no second `qwen3-4b-2` definition.
    let mut config = legacy_config(QWEN4B);
    config.definition_id = None;
    config.context_size = 4096;
    config.api_key_ref = None;
    domain
        .save_model_preset(QWEN4B, &ModelPreset::from_launch_config(&config))
        .unwrap();
    let defs = domain.definitions().unwrap();
    assert_eq!(
        defs.len(),
        1,
        "{:?}",
        defs.iter().map(|d| &d.alias).collect::<Vec<_>>()
    );
    assert_eq!(defs[0].id, made.id);
    assert_eq!(defs[0].alias, "qwen3-4b");
    assert_eq!(domain.dashboard_definition(QWEN4B).unwrap(), Some(made.id));
}

fn v2_definition(extra: serde_json::Value) -> serde_json::Value {
    let mut base = serde_json::json!({
        "base": {"kind": "local", "path": QWEN4B},
        "load": {"context_size": 16384, "gpu_layers": 33, "threads": 6},
        "auto_created": true, "created_at": 1, "updated_at": 1,
    });
    base.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    base
}

/// v3: values an old auto-fit wrote into a definition Harness Studio made are cleared; nothing a
/// user shaped is touched; the v2 file is backed up; a second load changes nothing.
#[test]
fn v3_clears_auto_written_load_values_of_untouched_auto_definitions_only() {
    let dir = tempfile::tempdir().unwrap();
    let stem = "Qwen3-4B-Instruct-2507-Q4_K_M";
    let kv = serde_json::json!({"context_size": 16384, "gpu_layers": 33, "cache_type_k": "q8_0"});
    let slots = serde_json::json!({"context_size": 16384, "gpu_layers": 33, "parallel_slots": 2});
    let v2 = serde_json::json!({
        "schema_version": 2,
        "definitions": [
            v2_definition(serde_json::json!({"id": "md-auto", "alias": stem})),
            v2_definition(serde_json::json!({"id": "md-auto2", "alias": format!("{stem}-2")})),
            v2_definition(serde_json::json!({"id": "md-edited", "alias": stem, "auto_created": false})),
            v2_definition(serde_json::json!({"id": "md-renamed", "alias": format!("{stem}-work")})),
            v2_definition(serde_json::json!({"id": "md-desc", "alias": stem, "description": "x"})),
            v2_definition(serde_json::json!({"id": "md-args", "alias": stem, "extra_args": "--foo"})),
            v2_definition(serde_json::json!({"id": "md-tpl", "alias": stem, "chat_template": "chatml"})),
            v2_definition(serde_json::json!({"id": "md-kv", "alias": stem, "load": kv})),
            v2_definition(serde_json::json!({"id": "md-slots", "alias": stem, "load": slots})),
            v2_definition(serde_json::json!({"id": "md-pinned", "alias": stem, "pinned": true})),
            // Made for a named server, not a model's Dashboard default.
            v2_definition(serde_json::json!({"id": "md-server", "alias": stem})),
        ],
        "model_defaults": {
            QWEN4B: {"definition_id": "md-auto"},
            "b": {"definition_id": "md-auto2"},
            "c": {"definition_id": "md-edited"},
            "d": {"definition_id": "md-renamed"},
            "e": {"definition_id": "md-desc"},
            "f": {"definition_id": "md-args"},
            "g": {"definition_id": "md-tpl"},
            "h": {"definition_id": "md-kv"},
            "i": {"definition_id": "md-slots"},
            "j": {"definition_id": "md-pinned"},
        },
    });
    let path = dir.path().join(DEFINITIONS_SCHEMA.file);
    let original = serde_json::to_vec_pretty(&v2).unwrap();
    fs::write(&path, &original).unwrap();

    let domain = Domain::at(dir.path());
    let defs = domain.definitions().unwrap();
    let get = |id: &str| defs.iter().find(|d| d.id == id).unwrap();
    for id in ["md-auto", "md-auto2"] {
        assert_eq!(get(id).load.get("context_size"), None, "{id}");
        assert_eq!(get(id).load.get("gpu_layers"), None, "{id}");
        assert_eq!(get(id).load.get("threads"), Some(&ParamValue::Int(6)));
        assert!(get(id).auto_created);
    }
    for id in [
        "md-edited",
        "md-renamed",
        "md-desc",
        "md-args",
        "md-tpl",
        "md-kv",
        "md-slots",
        "md-pinned",
        "md-server",
    ] {
        assert_eq!(
            get(id).load.get("context_size"),
            Some(&ParamValue::Int(16384)),
            "{id}"
        );
        assert_eq!(
            get(id).load.get("gpu_layers"),
            Some(&ParamValue::Int(33)),
            "{id}"
        );
    }

    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["schema_version"], 3);
    assert_eq!(
        fs::read(dir.path().join("model_definitions.json.bak-v2")).unwrap(),
        original
    );
    // Runs once: a later load changes nothing.
    let after = fs::read(&path).unwrap();
    domain.definitions().unwrap();
    assert_eq!(fs::read(&path).unwrap(), after);
}

/// profiles v2: the Dashboard server's last session loses the context, layer and slot count a
/// start froze into it; named profiles keep theirs; the v1 file is backed up; runs once.
#[test]
fn profiles_v2_clears_start_frozen_values_of_the_dashboard_server_only() {
    let dir = tempfile::tempdir().unwrap();
    let overrides = serde_json::json!({
        "context_size": 8192, "gpu_layers": 37, "parallel_slots": 1, "cache_type_k": "q8_0", "threads": 6,
    });
    let v1 = serde_json::json!({
        "schema_version": 1,
        "profiles": [
            {"id": "default", "name": "Dashboard", "definition_id": "md-1", "host": "127.0.0.1", "port": 8080,
             "serve_overrides": overrides, "created_at": 1, "updated_at": 1},
            {"id": "work", "name": "Work", "definition_id": "md-1", "host": "127.0.0.1", "port": 8080,
             "serve_overrides": overrides, "created_at": 1, "updated_at": 1},
        ],
    });
    let path = dir.path().join(PROFILES_SCHEMA.file);
    let original = serde_json::to_vec_pretty(&v1).unwrap();
    fs::write(&path, &original).unwrap();

    let doc = |p: &Path| -> serde_json::Value {
        serde_json::from_str(&fs::read_to_string(p).unwrap()).unwrap()
    };
    let domain = Domain::at(dir.path());
    domain.profiles().unwrap();
    let after = doc(&path);
    assert_eq!(after["schema_version"], 2);
    let ov = |id: &str| {
        after["profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .unwrap()["serve_overrides"]
            .clone()
    };
    assert_eq!(ov("default"), serde_json::json!({"threads": 6}));
    assert_eq!(ov("work"), overrides);
    assert_eq!(
        fs::read(dir.path().join("server_profiles.json.bak-v1")).unwrap(),
        original
    );
    let bytes = fs::read(&path).unwrap();
    domain.profiles().unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
