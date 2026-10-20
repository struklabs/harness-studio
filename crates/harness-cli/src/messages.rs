//! English messages for error codes.
//!
//! The core and the Control API return i18n keys (`models.in_use`, `api.not_found`...); the
//! GUI translates them with its locale files. The CLI prints English: the texts of the keys
//! the GUI knows are the GUI's English texts (a unit test compares them with
//! `src/lib/locales/*.ts`, so they cannot drift), the API-only and CLI-only codes have their
//! own texts here. The core's other keyed errors (`harness_core::error_key`) take their
//! English text from the GUI's error locale files (`src/lib/locales/errors*.ts`, compiled in),
//! `{name}` filled from the parameters. The few free-text core errors are printed as they are.
//! Unknown keys print the key itself plus its parameters, never nothing.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

/// The GUI's locale files of the core's keyed errors (a test checks the list is complete).
const ERROR_LOCALES: &[(&str, &str)] = &[
    (
        "errors.ts",
        include_str!("../../../src/lib/locales/errors.ts"),
    ),
    (
        "errorsDownloads.ts",
        include_str!("../../../src/lib/locales/errorsDownloads.ts"),
    ),
    (
        "errorsIntegrations.ts",
        include_str!("../../../src/lib/locales/errorsIntegrations.ts"),
    ),
    (
        "errorsRuntime.ts",
        include_str!("../../../src/lib/locales/errorsRuntime.ts"),
    ),
    (
        "errorsStore.ts",
        include_str!("../../../src/lib/locales/errorsStore.ts"),
    ),
    (
        "errorsRag.ts",
        include_str!("../../../src/lib/locales/errorsRag.ts"),
    ),
    (
        "errorsLink.ts",
        include_str!("../../../src/lib/locales/errorsLink.ts"),
    ),
    (
        "errorsProjects.ts",
        include_str!("../../../src/lib/locales/errorsProjects.ts"),
    ),
];

/// English template of a core error key from [`ERROR_LOCALES`].
pub fn locale_template(key: &str) -> Option<&'static str> {
    static MAP: OnceLock<HashMap<&'static str, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        ERROR_LOCALES
            .iter()
            .flat_map(|(_, src)| english_entries(src))
            // Generator errors live with the Create UI's translations.
            .chain(english_entries(include_str!(
                "../../../src/lib/locales/create.ts"
            )))
            // Conversion errors (`convert.*`) live with the conversion UI's translations.
            .chain(english_entries(include_str!(
                "../../../src/lib/locales/convert.ts"
            )))
            // mistral.rs errors (`mistralrs.*`) live with its local runtime card's translations.
            .chain(english_entries(include_str!(
                "../../../src/lib/locales/mistralrs.ts"
            )))
            .chain(english_entries(include_str!(
                "../../../src/lib/locales/browser.ts"
            )))
            .collect()
    })
    .get(key)
    .map(String::as_str)
}

/// `'key': 'text'` pairs of the English dictionary (`const en ... = { ... }`) of a locale
/// file. Texts are single- or double-quoted literals, possibly on the line after the key.
fn english_entries(src: &'static str) -> Vec<(&'static str, String)> {
    let Some(start) = src.find("const en") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = &src[start..];
    while let Some(pos) = rest.find("\n  '") {
        rest = &rest[pos + 4..];
        let Some(end) = rest.find("':") else { break };
        let key = &rest[..end];
        rest = rest[end + 2..].trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '\'' || *c == '"') else {
            continue;
        };
        let mut text = String::new();
        let mut chars = rest[1..].char_indices();
        let mut consumed = rest.len();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => match chars.next() {
                    Some((_, 'n')) => text.push('\n'),
                    Some((_, other)) => text.push(other),
                    None => break,
                },
                c if c == quote => {
                    consumed = i + 2;
                    break;
                }
                c => text.push(c),
            }
        }
        out.push((key, text));
        rest = &rest[consumed.min(rest.len())..];
    }
    out
}

/// Base text of a code (no parameters).
pub fn base(code: &str) -> Option<&'static str> {
    Some(match code {
        // Keys shared with the GUI (src/lib/network.ts ERROR_KEYS); texts = English locale.
        "network.lan_without_key" => "The server would bind to a network address without an API key. Set a key, use 127.0.0.1, or confirm \"Open Anyway\" for this server.",
        "network.tools_without_key" => "With tools/MCP enabled a server can’t be exposed to the network without a key (the model gets file and process access). Set an API key or use 127.0.0.1.",
        "network.invalid_host" => "Invalid bind address: only an IP address (e.g. 127.0.0.1, 0.0.0.0, 192.168.1.20) or localhost is allowed.",
        "network.invalid_origin" => "Invalid origin: use http(s)://host[:port], localhost or * (no path, comma or spaces).",
        "network.key_generation_failed" => "Could not generate a strong key.",
        "runtime.integrity_unverified" => "This runtime archive has no SHA-256 digest; installing it needs your explicit confirmation.",
        "api.network_not_allowed" => "The Control API would bind to a network address but \"Allow network access\" is off.",
        "api.network_without_token" => "Create at least one named token before opening the Control API to the network (the session token only exists on this computer).",
        "api.invalid_port" => "Invalid port: use a value between 1024 and 65535.",
        "api.invalid_origin" => "Invalid origin: write http(s)://host[:port] only (* and localhost are not accepted).",
        "api.token_name_invalid" => "The token name can’t be empty (64 characters at most).",
        "api.token_scopes_empty" => "Choose at least one scope.",
        "api.token_not_found" => "Token not found.",
        "api.random_failed" => "Could not generate a secure random value.",
        "api.bind_failed" => "The Control API could not start on this address (the port may be in use)",
        "api.not_owner" => "Another Harness instance manages the data directory; the Control API only runs in the owner.",
        "gateway.invalid_port" => "Invalid port: enter a value between 1024 and 65535.",
        "gateway.network_not_allowed" => "The gateway would bind a network address but \"Allow network access\" is off.",
        "gateway.network_without_token" => "Create a named token with the \"inference\" scope first (Settings > Developer > Control API) to open the gateway to the network. There is no keyless network access.",
        "gateway.invalid_origin" => "Invalid origin: only http(s)://host[:port] (* and localhost are not accepted).",
        "gateway.port_conflict" => "The gateway port cannot be the Control API port.",
        "gateway.not_owner" => "Another Harness instance manages the data directory; the gateway only runs in the owner.",
        "gateway.bind_failed" => "The gateway could not start on this address (the port may be in use)",
        "gateway.ollama_port_conflict" => "The Ollama port must be 1024 or higher and differ from the gateway and Control API ports.",
        "gateway.ollama_port_ollama" => "A real Ollama is running on this port; Harness does not stop it. The Ollama-compatible API stays on the gateway port only. To use the port, quit Ollama and turn \"Also listen on the Ollama port\" off and on again, or pick another port. Port",
        "gateway.ollama_port_harness" => "Another Harness gateway (Ollama-compatible) is running on this port. The Ollama-compatible API stays on this gateway’s port only. Port",
        "gateway.ollama_port_busy" => "Another program uses this port; the Ollama-compatible API stays on the gateway port only. Port",
        "gateway.ollama_port_installed" => "Ollama is installed on this computer; Harness does not take its port (Ollama could not start later). The Ollama-compatible API stays on the gateway port only. Uninstall Ollama or pick another port. Port",
        "models.in_use" => "The model is in use; stop the server first",
        "models.not_found" => "The model is not in the library.",
        "benchmark.already_running" => "A benchmark is already running.",
        "downloads.variant_not_found" => "The requested file or quantization is not in this repository.",
        "downloads.variant_ambiguous" => "Several files match: {files}. Name the file (CLI: --file <path>).",
        "definitions.alias_not_found" => "No model definition has this alias.",
        "servers.already_active" => "The server is already running or starting.",
        "servers.previous_still_running" => "The previous server process could not be stopped and is still running. Try again in a few seconds; if it persists, restart the app.",
        "servers.definition_required" => "Name a model definition to start.",
        "servers.profile_definition_mismatch" => "This profile serves another model definition.",
        "runtime.backend_no_device" => "This build detected no compatible GPU (the card may be unsupported, or the driver or CUDA/ROCm/oneAPI may be missing or incompatible). The current runtime is unchanged; you can install the Vulkan build.",
        "runtime.not_found" => "Runtime not found",
        "runtime.id_taken" => "Another runtime uses this id",
        "runtime.invalid_id" => "Invalid id (use lowercase letters, digits and -)",
        "runtime.no_server_binary" => "The folder has no llama-server",
        "runtime.unknown_family" => "This version does not know this runtime family",
        "runtime.family_not_installed" => "No runtime of this family is installed (add one with `harness runtime add --github <owner/repo> --family <family>`, or use the Install button in the app)",
        "runtime.family_mismatch" => "The chosen runtime does not run this model format (llama.cpp: GGUF, whisper.cpp: ggml .bin)",
        "process.sdcpp_loopback_only" => "sd-server has no authentication and only listens on loopback",
        "export.invalid_image" => "The image data is not a PNG, JPEG or WebP picture",
        "downloads.not_safetensors" => "The downloaded file is not a valid safetensors file",
        "downloads.not_checkpoint" => "The downloaded file is not a valid checkpoint",
        "lemonade.not_installed" => "Lemonade is not installed (run `harness lemonade install`, or use the Install button in Settings > Runtime > Registry)",
        "lemonade.start_failed" => "lemond did not start (its last output is in the message; the full log is <data>/lemonade/lemond.log)",
        "lemonade.start_timeout" => "lemond did not answer in time",
        "lemonade.api_error" => "Lemonade answered with an error",
        "lemonade.busy" => "Another Lemonade download is running",
        "lemonade.in_use" => "This Lemonade build is running; stop it first",
        "lemonade.bad_backend" => "Unknown llama.cpp backend (use auto, vulkan, rocm or cpu)",
        "lemonade.bad_model" => "Not a Lemonade model name or checkpoint",
        "lemonade.pull_incomplete" => "The download ended before it was complete",
        "process.lemonade_loopback_only" => "lemond only listens on loopback",
        "onnx.runtime_missing" => "ONNX Runtime is not installed (use the Install button in Settings > Runtime > Registry, or it is downloaded on first use)",
        "onnx.unsupported_platform" => "There is no such ONNX Runtime build for this platform",
        "onnx.platform_unsupported" => "ONNX Runtime has no build for this platform (Intel Macs among them), so ONNX models cannot run here; use the GGUF versions of embedding and rerank models",
        "onnx.genai_unsupported_platform" => "ONNX Runtime GenAI chat models only run on Windows for now",
        "onnx.sidecar_missing" => "harness-onnx was not found next to the app (reinstall, or set HARNESS_ONNX_BIN)",
        "process.onnx_loopback_only" => "harness-onnx has no authentication and only listens on loopback",
        "downloads.not_a_model_file" => "The downloaded file is not a model file (it may be a web page)",
        "process.whisper_loopback_only" => "whisper-server has no authentication and only listens on loopback",
        "downloads.not_ggml" => "The downloaded file is not a valid whisper ggml model",
        "downloads.custom_not_model" => "The file is neither a GGUF nor a whisper ggml model (its first bytes were not recognized)",
        "audio.ffmpeg_missing" => "This audio format needs ffmpeg and none was found",
        "audio.convert_failed" => "The audio file could not be converted",
        "audio.convert_timeout" => "Converting the audio timed out",
        "runtime.not_runnable" => "The engine binary could not be run",
        "runtime.changed" => "The runtime’s files changed since it was added; verify it in Settings > Runtime",
        "runtime.in_use" => "Runtime in use",
        "runtime.official_fixed" => "The official runtime can’t be removed",
        "runtime.quant_unsupported" => "The selected runtime can’t load this model’s quant type",
        "runtime.bad_repo" => "Invalid GitHub repository",
        "runtime.no_release" => "No matching release archive found",
        "runtime.whisper_no_macos" => "whisper.cpp publishes no server build for macOS (only an xcframework). Build whisper-server yourself and add it with \"Add local folder\".",
        "runtime.github_error" => "Could not reach GitHub",
        "servers.port_in_use" => "Port in use",
        "servers.alias_in_use" => "The --alias in the custom arguments is already served by another running server; pick a different name",
        "servers.no_failure" => "This server has no failure to recover from.",
        // Control API only.
        "api.unauthorized" => "The Control API rejected the token (missing, wrong or revoked).",
        "api.insufficient_scope" => "The token lacks the scope this command needs.",
        "api.server_key_local_only" => "Only this computer's own session can read a server's key.",
        "api.forbidden_host" => "The Control API refused the Host header.",
        "api.forbidden_origin" => "The Control API refused the browser Origin.",
        "api.no_route" => "The Control API has no such endpoint (version mismatch?).",
        "api.invalid_body" => "The Control API rejected the request body.",
        "api.invalid_query" => "The Control API rejected the query string.",
        "api.invalid_path" => "The Control API rejected the path.",
        "api.internal" => "Internal Control API error.",
        "api.definition_or_alias_required" => "Name a model definition (alias or id).",
        "api.alias_or_server_required" => "Name a model alias or a server.",
        "api.invalid_range" => "Unknown range; use 24h, 7d, 30d, 90d or all.",
        "api.shutdown_unsupported" => "This instance cannot be shut down remotely (it is the GUI; close it there).",
        "servers.exists" => "A server profile with this id already exists.",
        "benchmark.not_running" => "The benchmark is not running.",
        "runtime.not_available" => "No llama-server runtime is installed or configured.",
        // CLI.
        "cli.no_instance" => "No running Harness instance. Start one (`harness serve` or the GUI with the Control API on), or pass --headless to run a temporary one for this command.",
        "cli.owner_without_api" => "Another Harness process owns the data directory but its Control API is off. Turn it on in Settings > Developer > Control API (or close that process) and try again.",
        "cli.connection_failed" => "Could not reach the Harness Control API.",
        "cli.server_unreachable" => "Could not reach the llama-server endpoint.",
        "cli.confirmation_required" => "This command changes data; pass --yes to confirm when not running in a terminal.",
        "cli.aborted" => "Aborted.",
        "cli.lock_held" => "Another Harness process owns the data directory.",
        "cli.server_failed" => "The server failed to start.",
        "cli.out_of_memory" => "The model does not fit in memory (llama-server reported an allocation failure).",
        "cli.timeout" => "Timed out.",
        "cli.not_found" => "Not found.",
        "cli.invalid_argument" => "Invalid argument.",
        "cli.not_available" => "Not available yet.",
        "cli.io" => "File error.",
        "cli.service_failed" => "The OS service command failed.",
        "cli.chat_failed" => "The chat request failed.",
        "cli.server_key_unavailable" => "The server's key for this run is not available here.",
        "cli.download_failed" => "The download failed.",
        "cli.benchmark_failed" => "The benchmark failed.",
        _ => return None,
    })
}

/// English templates of the classified start failures (`failure.*`) and the recovery options
/// (`recovery.*`): the GUI's English texts (a test compares them), `{name}` placeholders.
pub fn template(key: &str) -> Option<&'static str> {
    Some(match key {
        "failure.killed" => "The {engine} server was ended from outside (SIGKILL): by the system when memory ran low, or by another program. If memory ran out, try a smaller model or quantization or a shorter context, or close other models and apps.",
        "failure.killed_by_system" => "Memory ran out: the system ended the {engine} server (SIGKILL). Try a smaller model or quantization or a shorter context, or close other models and apps.",
        "failure.out_of_memory" => "Not enough memory: the {engine} server could not allocate memory for the model (VRAM or RAM).",
        "failure.missing_library" => "A library the runtime needs could not be loaded (GPU driver or runtime files missing or incompatible).",
        "failure.corrupt_model" => "The model file is damaged or incomplete (interrupted download?). Download it again.",
        "failure.unsupported_architecture" => "This llama.cpp runtime does not support the model architecture \"{arch}\". Update the runtime.",
        "failure.model_not_found" => "The model file was not found or could not be opened.",
        "failure.port_in_use" => "The port is in use by another program. Choose another port.",
        "failure.invalid_argument" => "This runtime rejected a launch argument: {flag}. Remove it or update the runtime.",
        "failure.load_timeout" => "The server was not ready within {seconds} s. Raise the load timeout for large models.",
        "failure.crashed" => "The {engine} server stopped unexpectedly.",
        "failure.start_failed" => "The {engine} server could not start.",
        "failure.device_lost" => "The GPU stopped the work (driver reset or GPU error). Start the server again; if it repeats, try fewer GPU layers or another GPU driver version.",
        "failure.draft_load_failed" => "The draft model could not be loaded: {draft}. Usually it does not fit in memory: give the draft fewer GPU layers (-ngld) or turn the draft model off. If the file is missing or damaged, choose another draft.",
        "recovery.enable_fit" => "Let llama-server choose the GPU layers (--fit on)",
        "recovery.lower_context" => "Lower the context: {from} → {to}",
        "recovery.kv_cache_quant" => "Quantize the KV cache: {from} → {to} (~{saved_mib} MiB less)",
        "recovery.lower_gpu_layers" => "Fewer GPU layers: {from} → {to} of {total} (the rest on the CPU, slower)",
        "recovery.cpu_moe" => "Keep the MoE expert weights of {layers} layers on the CPU",
        "recovery.lower_ubatch" => "Smaller physical batch (-ub): {from} → {to}",
        "recovery.unload_server" => "Stop \"{server}\" first to free its GPU memory",
        "recovery.ram_insufficient" => "System memory may not be enough: the layers left on the CPU need ~{ram_need_mib} MiB, {ram_free_mib} MiB is free.",
        _ => return None,
    })
}

/// Replaces `{name}` with `params[name]` (unknown names stay as they are).
pub fn fill(template: &str, params: &Map<String, Value>) -> String {
    let mut out = template.to_string();
    for (k, v) in params {
        out = out.replace(&format!("{{{}}}", k), &value_text(v));
    }
    out
}

/// English text of a core error string (a keyed error is rendered, free text stays as it is).
pub fn core_text(text: &str) -> String {
    match harness_core::error_key::parse(text) {
        Some((key, params)) => render(key, &params),
        None => text.to_string(),
    }
}

/// Full message: base text plus the parameters that carry information. Parameters that are
/// keyed errors themselves (nested causes) are rendered first.
pub fn render(code: &str, params: &Map<String, Value>) -> String {
    render_depth(code, params, 0)
}

fn render_depth(code: &str, params: &Map<String, Value>, depth: usize) -> String {
    let params: Map<String, Value> = params
        .iter()
        .map(|(k, v)| {
            let v = match v.as_str().and_then(harness_core::error_key::parse) {
                Some((key, inner)) if depth < 3 => {
                    Value::String(render_depth(key, &inner, depth + 1))
                }
                _ => v.clone(),
            };
            (k.clone(), v)
        })
        .collect();
    let params = &params;
    let text = |k: &str| params.get(k).map(value_text);
    if let Some(t) = code
        .strip_prefix("servers.")
        .and_then(|kind| template(&format!("failure.{}", kind)))
    {
        return fill(t, params);
    }
    match code {
        // Free text from the core (already a sentence).
        "core.error" => text("message").unwrap_or_else(|| "Error".into()),
        "api.not_found" => format!(
            "{} not found: {}",
            text("resource").unwrap_or_else(|| "resource".into()),
            text("id").unwrap_or_default()
        ),
        "downloads.variant_ambiguous" => fill(base(code).unwrap_or_default(), params),
        "api.insufficient_scope" => format!(
            "{} (needs \"{}\")",
            base(code).unwrap_or_default(),
            text("scope").unwrap_or_default()
        ),
        _ if base(code).is_none() && locale_template(code).is_some() => {
            let template = locale_template(code).unwrap_or_default();
            let mut out = fill(template, params);
            if let Some(d) = text("detail").filter(|d| !d.is_empty()) {
                if !template.contains("{detail}") {
                    out = format!("{}: {}", out.trim_end_matches(['.', ':']), d);
                }
            }
            out
        }
        _ => {
            let mut out = base(code)
                .map(str::to_string)
                .unwrap_or_else(|| code.into());
            for key in ["detail", "message", "id"] {
                if let Some(v) = text(key).filter(|v| !v.is_empty()) {
                    out = format!("{} ({})", out.trim_end_matches('.'), v);
                    break;
                }
            }
            out
        }
    }
}

fn value_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Whether a text reports an out-of-memory failure (the core's marker list).
pub fn mentions_out_of_memory(text: &str) -> bool {
    harness_core::failure::mentions_out_of_memory(text)
}

/// Errors that mean "does not fit" get exit code 5.
pub fn is_out_of_memory(code: &str, params: &Map<String, Value>) -> bool {
    code == "cli.out_of_memory"
        || code == "servers.out_of_memory"
        || code == "servers.insufficient_memory"
        || code == "servers.mlx_insufficient_memory"
        || code == "process.gpu_out_of_memory"
        || params
            .values()
            .filter_map(Value::as_str)
            .any(mentions_out_of_memory)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// `'key': 'text'` (possibly spread over lines) of the English half of a locale file.
    fn english_text(key: &str) -> Option<String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/locales");
        let needle = format!("'{}':", key);
        for entry in std::fs::read_dir(dir).ok()? {
            let text = std::fs::read_to_string(entry.ok()?.path()).ok()?;
            // Turkish dictionary first, English second: the last occurrence is English.
            let Some(pos) = text.rfind(&needle) else {
                continue;
            };
            if text.matches(&needle).count() < 2 {
                continue;
            }
            let rest = text[pos + needle.len()..].trim_start();
            let (quote, body) = rest.split_at(1);
            let end = body.find(quote)?;
            return Some(body[..end].replace("\\'", "'"));
        }
        None
    }

    fn gui_error_keys() -> Vec<String> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/network.ts");
        let text = std::fs::read_to_string(path).unwrap();
        let start = text.find("ERROR_KEYS = [").unwrap();
        let end = start + text[start..].find("] as const").unwrap();
        text[start..end]
            .lines()
            .filter_map(|l| l.trim().strip_prefix('\''))
            .filter_map(|l| l.split('\'').next())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn gui_error_keys_have_the_gui_english_text() {
        let keys = gui_error_keys();
        assert!(keys.len() > 20, "{keys:?}");
        for key in keys {
            let cli = base(&key).unwrap_or_else(|| panic!("no CLI text for {key}"));
            let gui = english_text(&key).unwrap_or_else(|| panic!("no English locale for {key}"));
            assert_eq!(cli, gui, "{key}");
        }
    }

    #[test]
    fn failure_and_recovery_templates_are_the_gui_english_texts() {
        let keys = [
            "out_of_memory",
            "missing_library",
            "corrupt_model",
            "unsupported_architecture",
            "model_not_found",
            "port_in_use",
            "invalid_argument",
            "load_timeout",
            "crashed",
            "start_failed",
            "device_lost",
            "draft_load_failed",
        ]
        .map(|k| format!("failure.{k}"))
        .into_iter()
        .chain(
            [
                "enable_fit",
                "lower_context",
                "kv_cache_quant",
                "lower_gpu_layers",
                "cpu_moe",
                "lower_ubatch",
                "unload_server",
                "ram_insufficient",
            ]
            .map(|k| format!("recovery.{k}")),
        );
        for key in keys {
            let cli = template(&key).unwrap_or_else(|| panic!("no CLI template for {key}"));
            let gui = english_text(&key).unwrap_or_else(|| panic!("no English locale for {key}"));
            assert_eq!(cli, gui, "{key}");
        }
        // Every core failure kind has a template.
        for kind in harness_core::failure::FailureKind::ALL {
            assert!(template(&kind.message_key()).is_some(), "{}", kind.as_str());
        }
        let mut p = Map::new();
        p.insert("arch".into(), "fakearch".into());
        assert!(render("servers.unsupported_architecture", &p).contains("\"fakearch\""));
    }

    #[test]
    fn ambiguous_variant_lists_the_files() {
        let e: String = harness_core::error_key::err("downloads.variant_ambiguous")
            .with("files", "onnx/model.onnx, onnx/model_fp16.onnx")
            .into();
        let text = core_text(&e);
        assert!(
            text.contains("onnx/model.onnx, onnx/model_fp16.onnx"),
            "{text}"
        );
        assert!(text.contains("--file"), "{text}");
    }

    #[test]
    fn render_adds_parameters() {
        let mut p = Map::new();
        p.insert("detail".into(), "default, m-qwen".into());
        assert_eq!(
            render("models.in_use", &p),
            "The model is in use; stop the server first (default, m-qwen)"
        );
        assert_eq!(render("x.unknown", &Map::new()), "x.unknown");
        assert!(mentions_out_of_memory(
            "ggml_backend_cuda_buffer_type_alloc_buffer: allocating 9000 MiB on device 0: cudaMalloc failed: out of memory"
        ));
        assert!(!mentions_out_of_memory("server is listening"));
    }

    #[test]
    fn error_locale_list_is_complete() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/locales");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.starts_with("errors") && n.ends_with(".ts"))
            .collect();
        on_disk.sort();
        let mut listed: Vec<String> = ERROR_LOCALES.iter().map(|(n, _)| n.to_string()).collect();
        listed.sort();
        assert_eq!(on_disk, listed);
        // The compiled-in parser reads the same texts as the test's reader.
        for (_, src) in ERROR_LOCALES {
            for (key, text) in english_entries(src) {
                assert_eq!(Some(text), english_text(key), "{key}");
            }
        }
        assert_eq!(
            locale_template("fs.read_failed"),
            Some("Could not read the file ({path}): {error}")
        );
    }

    /// `err("a.b")` literals in a source tree.
    fn keys_used(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_used(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") && !path.ends_with("error_key.rs")
            {
                let text = std::fs::read_to_string(&path).unwrap();
                for (i, _) in text.match_indices("err(\"") {
                    let rest = &text[i + 5..];
                    let key = &rest[..rest.find('"').unwrap_or(0)];
                    if harness_core::error_key::is_key(key) {
                        out.push((key.to_string(), path.display().to_string()));
                    }
                }
            }
        }
    }

    #[test]
    fn every_core_error_key_has_texts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut used = Vec::new();
        for dir in [
            "crates/harness-core/src",
            "src-tauri/src",
            "crates/harness-api/src",
        ] {
            keys_used(&root.join(dir), &mut used);
        }
        assert!(used.len() > 10, "{used:?}");
        for (key, file) in used {
            assert!(
                base(&key).is_some() || locale_template(&key).is_some() || template(&key).is_some(),
                "{key} ({file}) has no English text in src/lib/locales/errors*.ts"
            );
            assert!(
                english_text(&key).is_some(),
                "{key} ({file}) has no GUI text"
            );
        }
    }

    #[test]
    fn registry_errors_have_cli_templates() {
        let error: String = harness_core::error_key::err("tools.registry_failed")
            .with("detail", "Registry cache unavailable")
            .into();
        assert_eq!(
            core_text(&error),
            "Could not complete the registry operation: Registry cache unavailable"
        );
        assert_eq!(core_text("tools.registry_soon"), "Coming Soon");
        for key in ["tools.registry_failed", "tools.registry_soon"] {
            assert_eq!(locale_template(key).map(str::to_string), english_text(key));
        }
    }

    #[test]
    fn creation_errors_have_cli_templates() {
        for key in [
            "create.js_invalid_string",
            "create.validation_failed",
            "create.tests_failed",
            "create.model_reply_truncated",
            "create.test_could_not_run",
            "create.retry_endpoint_required",
        ] {
            assert!(locale_template(key).is_some(), "{key}");
            assert!(english_text(key).is_some(), "{key}");
        }
    }

    #[test]
    fn transport_messages_keep_details_out_of_the_main_text() {
        for key in [
            "http.connect_failed",
            "http.connect_failed_local",
            "http.timeout",
            "http.timeout_local",
            "http.dns_failed",
            "http.tls_failed",
            "http.connection_closed",
            "http.invalid_response",
            "http.transport_failed",
        ] {
            let mut params = Map::new();
            params.insert(
                "url".into(),
                "http://127.0.0.1:12345/v1/chat/completions".into(),
            );
            params.insert("transport_detail".into(), "raw reqwest source chain".into());
            let text = render(key, &params);
            assert!(
                text.contains("http://127.0.0.1:12345/v1/chat/completions"),
                "{text}"
            );
            assert!(!text.contains("raw reqwest"), "{text}");
            assert_eq!(
                Some(text),
                locale_template(key).map(|t| t.replace("{url}", params["url"].as_str().unwrap()))
            );
        }
    }

    #[test]
    fn render_uses_locale_templates_and_nested_keys() {
        let inner: String = harness_core::error_key::err("fs.read_failed")
            .with("path", "C:/x.json")
            .with("error", "denied")
            .into();
        let (key, params) = harness_core::error_key::parse(&inner).unwrap();
        assert_eq!(
            render(key, &params),
            "Could not read the file (C:/x.json): denied"
        );
        let mut p = Map::new();
        p.insert("error".into(), Value::String(inner));
        assert_eq!(
            render("task.join_failed", &p),
            "A background task did not finish: Could not read the file (C:/x.json): denied"
        );
        let e = crate::error::CliError::core(String::from(
            harness_core::error_key::err("fs.not_found").with("path", "C:/m.gguf"),
        ));
        assert_eq!(e.message, "Not found: C:/m.gguf");
        assert_eq!(e.exit, crate::error::Exit::NotFound);
    }

    #[test]
    fn memory_refusals_exit_as_out_of_memory() {
        let none = Map::new();
        assert!(is_out_of_memory("servers.insufficient_memory", &none));
        assert!(is_out_of_memory("servers.mlx_insufficient_memory", &none));
        assert!(is_out_of_memory("process.gpu_out_of_memory", &none));
        assert!(!is_out_of_memory("servers.port_in_use", &none));
    }
}
