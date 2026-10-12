//! Integration writers against temporary HOME / APPDATA / CODEX_HOME / CLAUDE_CONFIG_DIR
//! directories (never the real user profile).

use super::*;
use tempfile::tempdir;

fn roots(dir: &Path) -> Roots {
    Roots {
        home: Some(dir.join("home")),
        appdata: Some(dir.join("appdata")),
        localappdata: Some(dir.join("localappdata")),
        claude_config_dir: None,
        codex_home: None,
        ledger: Some(dir.join("data").join("integrations.json")),
    }
}

fn req(model: &str) -> ApplyRequest {
    ApplyRequest {
        base_url: "http://127.0.0.1:11435/v1".into(),
        api_key: None,
        model_id: model.into(),
        model_name: model.into(),
        context_size: 65_536,
        target_path: None,
        models: Vec::new(),
        set_default_model: None,
    }
}

fn apply(r: &Roots, id: &str) -> Result<ApplyResult, String> {
    apply_in(r, id, &req("m1"))
}

fn oc_path(r: &Roots) -> PathBuf {
    r.home
        .as_ref()
        .unwrap()
        .join(".config")
        .join("opencode")
        .join("opencode.json")
}

fn write(path: &Path, content: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn read_json(path: &Path) -> Value {
    parse_tolerant(&std::fs::read_to_string(path).unwrap())
        .unwrap()
        .0
}

fn info(r: &Roots, id: &str) -> IntegrationInfo {
    list_integrations_in(r, "http://127.0.0.1:11435/v1", "m1", 65_536)
        .into_iter()
        .find(|i| i.id == id)
        .unwrap()
}

#[test]
fn nothing_is_detected_in_an_empty_profile() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    for i in list_integrations_in(&r, "http://x/v1", "m", 65_536) {
        assert!(!i.configured, "{}", i.id);
        assert!(i.config_paths.is_empty(), "{}", i.id);
        assert!(i.drifted.is_empty() && i.backups.is_empty(), "{}", i.id);
    }
}

#[test]
fn opencode_fresh_merge_idempotent_and_keeps_the_users_model() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    apply(&r, "opencode").unwrap();
    let v = read_json(&oc_path(&r));
    assert_eq!(v["model"], "harness/m1");
    assert_eq!(v["$schema"], "https://opencode.ai/config.json");
    assert_eq!(
        v["provider"]["harness"]["options"]["baseURL"],
        "http://127.0.0.1:11435/v1"
    );
    // Created by Harness: rollback deletes it again.
    let rev = revert_in(&r, "opencode").unwrap();
    assert_eq!(rev.deleted.len(), 1);
    assert!(!oc_path(&r).exists());

    let original = br#"{"theme":"dark","model":"anthropic/claude","provider":{"openai":{"name":"x"}},"mcp":{"a":1}}"#;
    write(&oc_path(&r), original);
    apply(&r, "opencode").unwrap();
    let once = std::fs::read(oc_path(&r)).unwrap();
    let again = apply(&r, "opencode").unwrap();
    assert!(
        again.outcome.written.is_empty(),
        "second apply writes nothing"
    );
    assert_eq!(std::fs::read(oc_path(&r)).unwrap(), once);
    let v = read_json(&oc_path(&r));
    assert_eq!(v["model"], "anthropic/claude", "the user's model stays");
    assert_eq!(v["theme"], "dark");
    assert_eq!(v["mcp"]["a"], 1);
    assert!(v["provider"]["harness"].is_object());
    // One from the rollback that deleted the created file, one from this apply.
    assert_eq!(keyed::backups_of(&oc_path(&r)).len(), 2);
    assert_eq!(model_opencode(&r).as_deref(), Some("m1"));
    revert_in(&r, "opencode").unwrap();
    let back: Value = serde_json::from_slice(&std::fs::read(oc_path(&r)).unwrap()).unwrap();
    let orig: Value = serde_json::from_slice(original).unwrap();
    assert_eq!(back, orig);
}

#[test]
fn jsonc_comments_and_trailing_commas_are_kept() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let jsonc = oc_path(&r).with_file_name("opencode.jsonc");
    write(
        &jsonc,
        b"// top comment\n{\n  /* block */ \"theme\": \"dark // not a comment\",\n  \"list\": [1, 2,],\n}\n",
    );
    apply(&r, "opencode").unwrap();
    assert!(!oc_path(&r).exists(), "writes the existing .jsonc");
    let text = std::fs::read_to_string(&jsonc).unwrap();
    assert!(text.contains("// top comment") && text.contains("/* block */"));
    let v = read_json(&jsonc);
    assert_eq!(v["theme"], "dark // not a comment");
    assert!(v["provider"]["harness"].is_object());
    assert!(info(&r, "opencode").configured);
    revert_in(&r, "opencode").unwrap();
    let text = std::fs::read_to_string(&jsonc).unwrap();
    assert!(text.contains("// top comment") && !text.contains("harness"));
}

#[test]
fn bom_is_accepted() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(br#"{"theme":"dark"}"#);
    write(&oc_path(&r), &bytes);
    apply(&r, "opencode").unwrap();
    let v = read_json(&oc_path(&r));
    assert_eq!(v["theme"], "dark");
    assert!(v["provider"]["harness"].is_object());
}

#[test]
fn malformed_files_are_left_untouched() {
    for id in ["opencode", "continue", "opendesign", "claude-code", "codex"] {
        let d = tempdir().unwrap();
        let r = roots(d.path());
        let home = r.home.clone().unwrap();
        let path = match id {
            "continue" => home.join(".continue").join("config.json"),
            "claude-code" => home.join(".claude").join("settings.json"),
            "codex" => home.join(".codex").join("config.toml"),
            _ => oc_path(&r),
        };
        let bad: &[u8] = b"{ \"theme\": \"dark\", oops not [json ";
        write(&path, bad);
        let err = apply(&r, id).unwrap_err();
        let key = crate::error_key::parse(&err).map(|(k, _)| k);
        assert_eq!(
            key,
            Some("integrations.config_unparseable"),
            "{}: {}",
            id,
            err
        );
        assert_eq!(std::fs::read(&path).unwrap(), bad, "{} modified file", id);
        assert!(keyed::backups_of(&path).is_empty());
        assert!(!r.ledger.as_ref().unwrap().exists());
    }
    // A non-object root is refused too.
    let d = tempdir().unwrap();
    let r = roots(d.path());
    write(&oc_path(&r), b"42");
    assert!(apply(&r, "opencode").is_err());
}

#[test]
fn continue_yaml_and_json() {
    let cases = [
        "models: []\n",
        "name: my\nmodels:\n- title: GPT\n  provider: openai\n  model: gpt-4o\n",
        "# comment\nschema: v1\n",
    ];
    for raw in cases {
        let d = tempdir().unwrap();
        let r = roots(d.path());
        let yaml = r.home.as_ref().unwrap().join(".continue/config.yaml");
        write(&yaml, raw.as_bytes());
        apply(&r, "continue").unwrap();
        apply(&r, "continue").unwrap();
        let value = parse_yaml(&std::fs::read_to_string(&yaml).unwrap()).unwrap();
        let models = value["models"].as_array().unwrap();
        assert_eq!(
            models
                .iter()
                .filter(|m| m["title"] == CONTINUE_TITLE)
                .count(),
            1,
            "case: {raw}"
        );
        if raw.contains("gpt-4o") {
            assert!(models.iter().any(|m| m["model"] == "gpt-4o"));
            assert_eq!(value["name"], "my");
        }
        assert_eq!(model_continue(&r).as_deref(), Some("m1"));
        revert_in(&r, "continue").unwrap();
        let back = parse_yaml(&std::fs::read_to_string(&yaml).unwrap()).unwrap();
        assert!(back["models"]
            .as_array()
            .map(|a| a.iter().all(|m| m["title"] != CONTINUE_TITLE))
            .unwrap_or(true));
    }
    // JSON is used only when there is no YAML.
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let json = r.home.as_ref().unwrap().join(".continue/config.json");
    write(
        &json,
        br#"{"models":[{"title":"Mine"}],"allowAnonymousTelemetry":false}"#,
    );
    apply(&r, "continue").unwrap();
    let v = read_json(&json);
    assert_eq!(v["models"].as_array().unwrap().len(), 2);
    assert_eq!(v["allowAnonymousTelemetry"], false);
    assert!(detect_continue(&r).configured);
}

#[test]
fn opendesign_updates_app_config_keeps_the_users_binary_and_writes_nothing_on_errors() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let od = r
        .appdata
        .as_ref()
        .unwrap()
        .join("Open Design/namespaces/release-stable-win/data/app-config.json");
    write(
        &od,
        br#"{"keep":true,"agentCliEnv":{"opencode":{"OPENCODE_BIN":"D:\\mine.exe"}}}"#,
    );
    apply(&r, "opendesign").unwrap();
    let v = read_json(&od);
    assert_eq!(v["keep"], true);
    assert_eq!(v["agentModels"]["opencode"]["model"], "harness/m1");
    assert_eq!(v["agentId"], "opencode");
    assert_eq!(v["agentCliEnv"]["opencode"]["OPENCODE_BIN"], "D:\\mine.exe");
    assert!(oc_path(&r).exists());
    assert!(detect_opendesign(&r).configured);
    assert_eq!(model_opendesign(&r).as_deref(), Some("m1"));
    revert_in(&r, "opendesign").unwrap();
    let v = read_json(&od);
    assert!(v.get("agentId").is_none() && v.get("agentModels").is_none());
    assert_eq!(v["keep"], true);

    // Broken app-config: error, nothing written (not even OpenCode).
    let d2 = tempdir().unwrap();
    let r2 = roots(d2.path());
    let od2 = r2
        .appdata
        .as_ref()
        .unwrap()
        .join("Open Design/namespaces/default/data/app-config.json");
    write(&od2, b"{ broken");
    let e = apply(&r2, "opendesign").unwrap_err();
    let (key, params) = crate::error_key::parse(&e).unwrap();
    assert_eq!(key, "integrations.config_unparseable");
    assert!(params["path"]
        .as_str()
        .unwrap()
        .ends_with("app-config.json"));
    assert_eq!(std::fs::read(&od2).unwrap(), b"{ broken");
    assert!(!oc_path(&r2).exists());
}

#[test]
fn claude_code_env_block_restores_the_users_values() {
    let d = tempdir().unwrap();
    let mut r = roots(d.path());
    // CLAUDE_CONFIG_DIR is honoured.
    let dir = d.path().join("claude-cfg");
    r.claude_config_dir = Some(dir.clone());
    let settings = dir.join("settings.json");
    write(
        &settings,
        b"{\n  // mine\n  \"model\": \"opus\",\n  \"env\": { \"ANTHROPIC_MODEL\": \"claude-x\", \"FOO\": \"1\" }\n}\n",
    );
    let mut q = req("qwen3-4b");
    q.api_key = Some("hs_named_token_value".into());
    apply_in(&r, "claude-code", &q).unwrap();
    let v = read_json(&settings);
    assert_eq!(v["env"]["ANTHROPIC_BASE_URL"], "http://127.0.0.1:11435");
    assert_eq!(v["env"]["ANTHROPIC_AUTH_TOKEN"], "hs_named_token_value");
    assert_eq!(v["env"]["ANTHROPIC_MODEL"], "qwen3-4b");
    assert_eq!(v["env"]["CLAUDE_CODE_MAX_CONTEXT_TOKENS"], "65536");
    assert_eq!(v["env"]["FOO"], "1");
    assert_eq!(v["model"], "opus");
    assert_eq!(model_claude(&r).as_deref(), Some("qwen3-4b"));
    // The ledger stores hashes of what Harness wrote, never the token itself.
    let ledger = std::fs::read_to_string(r.ledger.as_ref().unwrap()).unwrap();
    assert!(!ledger.contains("hs_named_token_value"));
    let rev = revert_in(&r, "claude-code").unwrap();
    assert!(rev.warnings.is_empty(), "{:?}", rev.warnings);
    let v = read_json(&settings);
    assert_eq!(v["env"], json!({"ANTHROPIC_MODEL": "claude-x", "FOO": "1"}));
    assert!(std::fs::read_to_string(&settings)
        .unwrap()
        .contains("// mine"));
    assert!(!info(&r, "claude-code").configured);
}

#[test]
fn codex_provider_table_and_profile_file() {
    let d = tempdir().unwrap();
    let mut r = roots(d.path());
    let home = d.path().join("codex-home");
    r.codex_home = Some(home.clone());
    let cfg = home.join("config.toml");
    write(
        &cfg,
        b"# my codex config\nmodel = \"gpt-5\"\n\n[mcp_servers.harness]\ncommand = \"harness\"\nargs = [\"mcp\"]\n",
    );
    apply(&r, "codex").unwrap();
    let text = std::fs::read_to_string(&cfg).unwrap();
    assert!(text.contains("# my codex config"));
    let parsed: toml::Value = toml::from_str(&text).unwrap();
    let p = &parsed["model_providers"]["harness"];
    assert_eq!(p["base_url"].as_str(), Some("http://127.0.0.1:11435/v1"));
    assert_eq!(p["wire_api"].as_str(), Some("responses"));
    assert_eq!(
        parsed["model"].as_str(),
        Some("gpt-5"),
        "default model untouched"
    );
    let profile = std::fs::read_to_string(home.join("harness.config.toml")).unwrap();
    let prof: toml::Value = toml::from_str(&profile).unwrap();
    assert_eq!(prof["model_provider"].as_str(), Some("harness"));
    assert_eq!(prof["model"].as_str(), Some("m1"));
    assert_eq!(prof["model_context_window"].as_integer(), Some(65_536));
    assert_eq!(model_codex(&r).as_deref(), Some("m1"));
    let i = info(&r, "codex");
    assert!(i.configured && i.drifted.is_empty());
    revert_in(&r, "codex").unwrap();
    assert!(!home.join("harness.config.toml").exists());
    let back: toml::Value = toml::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
    assert!(back.get("model_providers").is_none());
    assert!(back["mcp_servers"]["harness"].is_table(), "MCP entry stays");
}

/// Paths checked against the official docs and a real install (docs/en/INTEGRATIONS.md section 8):
/// VS Code `%APPDATA%/Code/User`, Kilo `~/.config/kilo` (also on Windows), Zed's key variable.
#[test]
fn verified_client_paths_and_zed_key_variable() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    assert_eq!(
        vscode_models_path(&r).unwrap(),
        d.path().join("appdata/Code/User/chatLanguageModels.json")
    );
    // `~/.config/kilo`; on Linux the config root is the XDG one (the `appdata` root there).
    let kilo_dir = if cfg!(target_os = "linux") {
        d.path().join("appdata/kilo")
    } else {
        d.path().join("home/.config/kilo")
    };
    assert_eq!(kilo_config_path(&r).unwrap(), kilo_dir.join("kilo.json"));
    // An existing kilo.jsonc wins over the default kilo.json.
    let jsonc = kilo_dir.join("kilo.jsonc");
    write(&jsonc, b"{}");
    assert_eq!(kilo_config_path(&r).unwrap(), jsonc);
    // Zed derives the variable from the provider name: UPPER_SNAKE + `_API_KEY`.
    let derived = ENTRY_NAME.to_uppercase().replace(' ', "_") + "_API_KEY";
    assert_eq!(derived, "HARNESS_STUDIO_API_KEY");
    assert!(after_apply_note("zed", &req("m1"))
        .unwrap()
        .contains(&derived));
    assert!(after_apply_note("zed", &req("m1"))
        .unwrap()
        .contains("harness-local"));
    assert!(!after_apply_note("zed", &req("m1"))
        .unwrap()
        .contains("isterse"));
    // The key itself never lands in VS Code's or Zed's file (secret storage / UI / env).
    let mut q = req("m1");
    q.api_key = Some("secret-token-123".into());
    apply_in(&r, "vscode-copilot", &q).unwrap();
    apply_in(&r, "zed", &q).unwrap();
    for p in [
        vscode_models_path(&r).unwrap(),
        zed_settings_path(&r).unwrap(),
    ] {
        assert!(!std::fs::read_to_string(p)
            .unwrap()
            .contains("secret-token-123"));
    }
}

#[test]
fn vscode_zed_kilo_goose_and_librechat() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let appdata = r.appdata.clone().unwrap();

    // VS Code: a root array; entries of other providers stay.
    let vs = appdata.join("Code/User/chatLanguageModels.json");
    write(
        &vs,
        br#"[{"name":"Mine","vendor":"customendpoint","models":[]}]"#,
    );
    apply(&r, "vscode-copilot").unwrap();
    let v = read_json(&vs);
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[1]["name"], ENTRY_NAME);
    assert_eq!(
        v[1]["models"][0]["url"],
        "http://127.0.0.1:11435/v1/chat/completions"
    );
    assert_eq!(model_vscode(&r).as_deref(), Some("m1"));
    revert_in(&r, "vscode-copilot").unwrap();
    assert_eq!(read_json(&vs).as_array().unwrap().len(), 1);

    // Zed: settings with comments.
    let zed = zed_settings_path(&r).unwrap();
    write(&zed, b"// Zed settings\n{\n  \"theme\": \"One Dark\"\n}\n");
    apply(&r, "zed").unwrap();
    let v = read_json(&zed);
    assert_eq!(
        v["language_models"]["openai_compatible"][ENTRY_NAME]["api_url"],
        "http://127.0.0.1:11435/v1"
    );
    assert_eq!(model_zed(&r).as_deref(), Some("m1"));
    revert_in(&r, "zed").unwrap();
    let text = std::fs::read_to_string(&zed).unwrap();
    assert!(text.contains("// Zed settings") && !text.contains("language_models"));

    // Kilo Code: new file with the Kilo schema, removed again.
    apply(&r, "kilo").unwrap();
    let kilo = kilo_config_path(&r).unwrap();
    let v = read_json(&kilo);
    assert_eq!(v["$schema"], "https://app.kilo.ai/config.json");
    assert_eq!(v["model"], "harness/m1");
    assert_eq!(model_kilo(&r).as_deref(), Some("m1"));
    revert_in(&r, "kilo").unwrap();
    assert!(!kilo.exists());

    // Goose: a file Harness owns.
    let mut q = req("m1");
    q.api_key = Some("secret-token-123".into());
    apply_in(&r, "goose", &q).unwrap();
    let gp = goose_provider_path(&r).unwrap();
    let v = read_json(&gp);
    assert_eq!(v["engine"], "openai");
    assert_eq!(v["base_url"], "http://127.0.0.1:11435/v1/chat/completions");
    assert_eq!(v["api_key_env"], "HARNESS_API_KEY");
    assert!(!std::fs::read_to_string(&gp)
        .unwrap()
        .contains("secret-token-123"));
    assert!(detect_goose(&r).configured);
    revert_in(&r, "goose").unwrap();
    assert!(!gp.exists());

    // LibreChat: needs a path.
    let e = apply(&r, "librechat").unwrap_err();
    assert_eq!(
        crate::error_key::parse(&e).map(|(k, _)| k),
        Some("integrations.librechat_path_missing")
    );
    let lc = d.path().join("lc").join("librechat.yaml");
    write(
        &lc,
        b"version: 1.3.17\nendpoints:\n  custom:\n    - name: Other\n      baseURL: http://x\n",
    );
    let mut q = req("m1");
    q.target_path = Some(lc.to_string_lossy().to_string());
    apply_in(&r, "librechat", &q).unwrap();
    let v = parse_yaml(&std::fs::read_to_string(&lc).unwrap()).unwrap();
    assert_eq!(v["endpoints"]["custom"][1]["name"], ENTRY_NAME);
    assert_eq!(
        v["endpoints"]["custom"][1]["baseURL"],
        "http://127.0.0.1:11435/v1"
    );
    let i = info(&r, "librechat");
    assert!(i.configured && i.config_paths.contains(&lc.to_string_lossy().to_string()));
    revert_in(&r, "librechat").unwrap();
    let v = parse_yaml(&std::fs::read_to_string(&lc).unwrap()).unwrap();
    assert_eq!(v["endpoints"]["custom"].as_array().unwrap().len(), 1);
}

#[test]
fn vscode_empty_array_is_detected_then_configured_in_current_byok_format() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let path = vscode_models_path(&r).unwrap();
    write(&path, b"[]");
    let before = list_integrations_in(&r, &req("m1").base_url, "m1", 65_536);
    let before = before.iter().find(|i| i.id == "vscode-copilot").unwrap();
    assert!(before.installed);
    assert!(!before.configured);
    apply(&r, "vscode-copilot").unwrap();
    let v = read_json(&path);
    assert_eq!(v[0]["name"], ENTRY_NAME);
    assert_eq!(v[0]["vendor"], "customendpoint");
    assert_eq!(v[0]["apiType"], "chat-completions");
    assert_eq!(v[0]["models"][0]["id"], "m1");
    assert_eq!(
        v[0]["models"][0]["url"],
        "http://127.0.0.1:11435/v1/chat/completions"
    );
    assert_eq!(v[0]["models"][0]["toolCalling"], true);
    assert!(v[0]["models"][0]["maxInputTokens"].as_u64().unwrap() > 0);
    assert!(v[0]["models"][0]["maxOutputTokens"].as_u64().unwrap() > 0);
    let mut q = req("m2");
    q.api_key = Some("gateway-secret".into());
    apply_in(&r, "vscode-copilot", &q).unwrap();
    let v = read_json(&path);
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["apiKey"], "${input:harnessStudioApiKey}");
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("gateway-secret"));
    revert_in(&r, "vscode-copilot").unwrap();
    assert_eq!(read_json(&path), json!([]));
}

#[test]
fn drift_is_reported_and_user_edits_survive_rollback() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    apply(&r, "opencode").unwrap();
    let path = oc_path(&r);
    let edited = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"harness/m1\"", "\"harness/m1-edited\"");
    std::fs::write(&path, edited).unwrap();
    let i = info(&r, "opencode");
    assert_eq!(i.drifted, vec![path.to_string_lossy().to_string()]);
    let rev = revert_in(&r, "opencode").unwrap();
    assert!(rev.warnings.iter().any(|w| w.contains("değiştirilmiş")));
    let v = read_json(&path);
    assert_eq!(v["model"], "harness/m1-edited", "the user's edit stays");
    assert!(v["provider"].get("harness").is_none());
}

#[test]
fn legacy_configs_without_a_ledger_are_reverted_by_marker() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    // What the earlier writer left behind.
    write(
        &oc_path(&r),
        br#"{"provider":{"harness":{"npm":"x"},"other":{}},"model":"harness/old"}"#,
    );
    assert!(info(&r, "opencode").configured);
    revert_in(&r, "opencode").unwrap();
    let v = read_json(&oc_path(&r));
    assert_eq!(v["provider"], json!({"other": {}}));
}

#[test]
fn guides_do_not_write_files() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    for id in [
        "cursor",
        "claude-desktop",
        "aider",
        "jan",
        "obsidian-copilot",
    ] {
        assert!(apply(&r, id).is_err(), "{id}");
        assert!(revert_in(&r, id).is_err(), "{id}");
        let i = info(&r, id);
        assert_eq!(i.kind, IntegrationKind::Guide);
        assert!(!i.can_apply);
    }
    assert_eq!(info(&r, "aider").launch, Some(LaunchTool::Aider));
    assert_eq!(info(&r, "claude-desktop").test_api, WireApi::Anthropic);
    assert_eq!(info(&r, "codex").test_api, WireApi::Responses);
}

#[test]
fn preview_writes_nothing() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    write(&oc_path(&r), br#"{"theme":"dark"}"#);
    let before = std::fs::read(oc_path(&r)).unwrap();
    for id in [
        "opencode",
        "claude-code",
        "codex",
        "zed",
        "vscode-copilot",
        "goose",
        "kilo",
    ] {
        let changes = preview_in(&r, id, &req("m1")).unwrap();
        assert!(!changes.is_empty(), "{id}");
    }
    assert_eq!(std::fs::read(oc_path(&r)).unwrap(), before);
    assert!(!r.ledger.as_ref().unwrap().exists());
}

#[test]
fn opencode_lists_every_gateway_model_and_changes_the_default_only_when_asked() {
    let d = tempdir().unwrap();
    let r = roots(d.path());
    let original = br#"{"model":"anthropic/claude-sonnet-4","theme":"dark"}"#;
    write(&oc_path(&r), original);
    let q = ApplyRequest {
        models: vec![
            ProviderModel {
                id: "qwen3-4b".into(),
                context_size: 131_072,
            },
            // Listed once, with the chosen model's context.
            ProviderModel {
                id: "M1".into(),
                context_size: 8_192,
            },
            ProviderModel {
                id: "tiny".into(),
                context_size: 0,
            },
        ],
        ..req("m1")
    };
    // Default: the user's own model stays.
    let changes = preview_in(&r, "opencode", &q).unwrap();
    let after: Value = parse_tolerant(&changes[0].after).unwrap().0;
    assert_eq!(after["model"], "anthropic/claude-sonnet-4");
    let models = after["provider"]["harness"]["models"].as_object().unwrap();
    assert_eq!(
        models.keys().cloned().collect::<Vec<_>>(),
        vec!["m1", "qwen3-4b", "tiny"],
        "the chosen model first, no duplicates"
    );
    assert_eq!(models["qwen3-4b"]["limit"]["context"], 131_072);
    assert_eq!(models["tiny"]["limit"]["context"], 65_536);
    assert_eq!(models["m1"]["tool_call"], true);
    assert_eq!(
        info(&r, "opencode").default_model.as_deref(),
        Some("anthropic/claude-sonnet-4")
    );

    // Opted in: the default moves to Harness; rollback restores the user's.
    apply_in(
        &r,
        "opencode",
        &ApplyRequest {
            set_default_model: Some(true),
            ..q.clone()
        },
    )
    .unwrap();
    assert_eq!(read_json(&oc_path(&r))["model"], "harness/m1");
    revert_in(&r, "opencode").unwrap();
    let back = read_json(&oc_path(&r));
    assert_eq!(back["model"], "anthropic/claude-sonnet-4");
    assert!(back.get("provider").is_none());

    // Opted out while the default already points at Harness: left as it is.
    write(&oc_path(&r), br#"{"model":"harness/old"}"#);
    apply_in(
        &r,
        "opencode",
        &ApplyRequest {
            set_default_model: Some(false),
            ..q.clone()
        },
    )
    .unwrap();
    assert_eq!(read_json(&oc_path(&r))["model"], "harness/old");
    // Not asked: our own default is refreshed.
    apply_in(&r, "opencode", &q).unwrap();
    assert_eq!(read_json(&oc_path(&r))["model"], "harness/m1");
}

#[test]
fn every_chat_writer_receives_only_chat_models() {
    use crate::domain::{ModelDefinition, ModelRef};
    let dir = tempdir().unwrap();
    let r = roots(dir.path());
    let defs: Vec<_> = ["chat", "speech", "embed", "rerank"]
        .iter()
        .map(|alias| {
            let mut d = ModelDefinition::new(
                *alias,
                ModelRef::Local {
                    path: dir.path().join(format!("{alias}.gguf")),
                },
            );
            if *alias != "chat" {
                let mode = match *alias {
                    "speech" => "transcription",
                    "embed" => "embedding",
                    _ => "rerank",
                };
                d.load.insert(
                    "serve_mode".into(),
                    crate::params::ParamValue::Text(mode.into()),
                );
            }
            d
        })
        .collect();
    let mut q = req("chat");
    q.models = defs
        .iter()
        .map(|d| ProviderModel {
            id: d.alias.clone(),
            context_size: 32768,
        })
        .collect();
    let filtered = filter_chat_request(&q, &defs, |_| None).unwrap();
    assert_eq!(
        filtered
            .models
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        vec!["chat"]
    );
    for id in [
        "opencode",
        "opendesign",
        "kilo",
        "zed",
        "vscode-copilot",
        "continue",
        "claude-code",
        "codex",
        "goose",
    ] {
        let changes = preview_in(&r, id, &filtered).unwrap();
        assert!(!changes.is_empty(), "{id}");
        assert!(changes.iter().any(|c| c.after.contains("chat")), "{id}");
        for c in changes {
            for excluded in ["speech", "embed", "rerank"] {
                assert!(!c.after.contains(excluded), "{id}: {excluded}");
            }
        }
    }
    for id in ["speech", "embed", "rerank"] {
        q.model_id = id.into();
        assert!(filter_chat_request(&q, &defs, |_| None)
            .unwrap_err()
            .contains("integrations.chat_model_required"));
    }
}

#[test]
fn chat_writer_filter_recognises_served_file_names_and_unregistered_speech() {
    use crate::domain::{ModelDefinition, ModelRef};
    let dir = tempdir().unwrap();
    let speech = dir.path().join("ggml-base.en.bin");
    std::fs::write(&speech, 0x6767_6d6cu32.to_le_bytes()).unwrap();
    let d = ModelDefinition::new(
        "asr-alias",
        ModelRef::Local {
            path: speech.clone(),
        },
    );
    for id in ["asr-alias", "ggml-base.en", "GGML-BASE.EN.BIN"] {
        assert!(filter_chat_request(&req(id), std::slice::from_ref(&d), |_| None).is_err());
    }
    assert!(filter_chat_request(&req(speech.to_str().unwrap()), &[], |_| None).is_err());
}
