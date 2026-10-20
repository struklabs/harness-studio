// Playground > Image > ComfyUI: the user's own ComfyUI (HTTP API only; Harness never installs
// or runs it). Run pictures are saved to `<data dir>/outputs/comfyui`.

use harness_core::comfyui::{self as comfy, ComfyClient};

/// A finished run: the pictures as saved, with their bytes for the gallery.
#[derive(serde::Serialize)]
pub(crate) struct ComfyRunResult {
    prompt_id: String,
    seed: u64,
    images: Vec<ComfyRunImage>,
}

#[derive(serde::Serialize)]
pub(crate) struct ComfyRunImage {
    path: String,
    format: String,
    /// Base64 of the file.
    data: String,
}

#[tauri::command]
pub(crate) async fn comfyui_get_settings() -> Result<comfy::ComfySettings, String> {
    blocking(comfy::settings).await
}

#[tauri::command]
pub(crate) async fn comfyui_save_settings(url: String) -> Result<comfy::ComfySettings, String> {
    blocking(move || comfy::save(&url)).await?
}

/// The detect check for a form's (possibly unsaved) address; empty uses the saved one.
#[tauri::command]
pub(crate) async fn comfyui_status(url: Option<String>) -> Result<comfy::ComfyStatus, String> {
    let client = blocking(move || ComfyClient::from_form(url.as_deref())).await??;
    client.status().await
}

#[tauri::command]
pub(crate) async fn comfyui_catalog() -> Result<comfy::Catalog, String> {
    blocking(ComfyClient::saved).await??.catalog().await
}

/// Runs a workflow on the saved ComfyUI; `comfyui-progress` events follow it. Resolves when
/// the pictures are saved, or fails with `comfyui.cancelled` after [`comfyui_cancel`].
#[tauri::command]
pub(crate) async fn comfyui_run(
    app: tauri::AppHandle,
    source: comfy::WorkflowSource,
    params: comfy::Txt2ImgParams,
) -> Result<ComfyRunResult, String> {
    use base64::Engine as _;
    let client = blocking(ComfyClient::saved).await??;
    let seed = params.seed.unwrap_or_else(comfy::random_seed);
    let graph = client.prepare(&source, &params, seed).await?;
    let outcome = client
        .run(&graph, &move |e| {
            let _ = app.emit("comfyui-progress", e);
        })
        .await?;
    let images = outcome.images;
    let saved = blocking({
        let images = images.clone();
        move || comfy::save_images(&comfy::outputs_dir(), &images)
    })
    .await??;
    Ok(ComfyRunResult {
        prompt_id: outcome.prompt_id,
        seed,
        images: saved
            .into_iter()
            .zip(images)
            .map(|(s, i)| ComfyRunImage {
                path: s.path,
                format: s.format,
                data: base64::engine::general_purpose::STANDARD.encode(&i.bytes),
            })
            .collect(),
    })
}

#[tauri::command]
pub(crate) async fn comfyui_cancel(prompt_id: String) -> Result<(), String> {
    blocking(ComfyClient::saved).await??.cancel(&prompt_id).await
}

/// Opens a saved run picture with the system's default app (only files in the outputs folder).
#[tauri::command]
pub(crate) async fn comfyui_open_output(app: tauri::AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let file = blocking(move || comfy::openable_output(&path)).await??;
    app.opener()
        .open_path(file.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn comfyui_reveal_output(path: String) -> Result<(), String> {
    blocking(move || {
        let file = comfy::openable_output(&path)?;
        harness_core::library::ops::reveal_in_folder(&file)
    })
    .await?
}
