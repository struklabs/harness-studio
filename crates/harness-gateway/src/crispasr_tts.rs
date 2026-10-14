//! Closed speech surface: preset voices only; no cloning, paths, or worker control fields.
use crate::errors::GatewayError;
use harness_core::domain::ModelDefinition;
use serde_json::{json, Value};

fn bad(message: &str) -> GatewayError {
    GatewayError::new(400, "invalid_request", message)
}

pub fn body(
    def: &ModelDefinition,
    method: &str,
    path: &str,
    bytes: &[u8],
) -> Result<Value, GatewayError> {
    if method != "POST" || path != "/v1/audio/speech" {
        return Err(bad("CrispASR TTS only accepts POST /v1/audio/speech"));
    }
    let model = def
        .base
        .local_path()
        .ok_or_else(|| bad("A local TTS model is required"))?;
    let voices = harness_core::engine::crispasr_tts::voices(model);
    if harness_core::engine::crispasr_tts::backend(model).is_none() {
        return Err(bad("This CrispASR TTS backend is not supported yet"));
    }
    let mut out = rebuild(
        &def.alias,
        &voices,
        harness_core::engine::crispasr_tts::backend(model) == Some("kokoro"),
        bytes,
    )?;
    if harness_core::engine::crispasr_tts::backend(model) == Some("kokoro") {
        use harness_core::engine::crispasr_helpers::{CMUDICT, MISAKI};
        if MISAKI.verified().is_none() || CMUDICT.verified().is_none() {
            return Err(bad("Download the pronunciation dictionaries in Settings > CrispASR Helper Models first"));
        }
    }
    // v0.8.43 advertises the loaded model path as its model id. This path comes solely
    // from the saved definition, never from the request; only this final object is sent.
    out["model"] = json!(model.to_string_lossy());
    Ok(out)
}

pub fn rebuild(
    alias: &str,
    voices: &[String],
    kokoro: bool,
    bytes: &[u8],
) -> Result<Value, GatewayError> {
    if bytes.len() > 32 * 1024 {
        return Err(bad("Speech JSON exceeds 32 KiB"));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| bad("Speech body must be JSON"))?;
    let object = value
        .as_object()
        .ok_or_else(|| bad("Speech body must be an object"))?;
    if object
        .keys()
        .any(|k| !["model", "input", "voice", "response_format", "speed"].contains(&k.as_str()))
    {
        return Err(bad(
            "Unknown speech field; voice cloning and consent fields are not supported",
        ));
    }
    if let Some(model) = object.get("model") {
        let model = model
            .as_str()
            .ok_or_else(|| bad("model must be an alias"))?;
        if model.contains(['/', '\\', ':']) || model.contains("..") || model.ends_with(".gguf") {
            return Err(bad("model must be an alias"));
        }
    }
    let input = value
        .get("input")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("input must be text"))?;
    if input.trim().is_empty() || input.chars().count() > 4000 || input.contains('\0') {
        return Err(bad("input must contain 1 to 4000 characters"));
    }
    let voice = match value.get("voice") {
        None => voices
            .first()
            .map(String::as_str)
            .ok_or_else(|| bad("Download this model's voice first (Settings > Installed Runtime > Other Runtimes > CrispASR Helper Models). / Önce bu modelin sesini indirin (Ayarlar > Kurulu Model Motoru > Diğer Model Motorları > CrispASR Yardımcı Modelleri)."))?,
        Some(v) => v
            .as_str()
            .ok_or_else(|| bad("voice must be a preset name; inline audio is not supported"))?,
    };
    if voice.contains(['/', '\\', ':'])
        || voice.contains("..")
        || voice.chars().any(char::is_control)
        || !voices.iter().any(|v| v == voice)
    {
        return Err(bad("voice must be one of this model's verified presets"));
    }
    let format = match value.get("response_format") {
        None => "wav",
        Some(v) => v.as_str().ok_or_else(|| bad("Invalid response_format"))?,
    };
    if !["wav", "pcm", "mp3", "opus", "aac", "flac"].contains(&format) {
        return Err(bad("Invalid response_format"));
    }
    let speed = match value.get("speed") {
        None => 1.0,
        Some(v) => v.as_f64().ok_or_else(|| bad("speed must be a number"))?,
    };
    if !(0.25..=4.0).contains(&speed) || (!kokoro && speed != 1.0) {
        return Err(bad(
            "Unsupported speed; Kokoro accepts 0.25 to 4, Pocket TTS requires 1",
        ));
    }
    let mut out = json!({"model":alias,"input":input,"response_format":"wav"});
    // Kokoro reads a filename verbatim. Its single pinned preset is a launch-owned path;
    // omitting voice inherits --voice. Pocket resolves its verified bare name in --voice-dir.
    if !kokoro {
        out["voice"] = json!(voice);
    }
    if speed != 1.0 {
        out["speed"] = json!(speed);
    }
    Ok(out)
}
