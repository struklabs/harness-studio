// Playground > Image > ComfyUI: the user's own ComfyUI (HTTP API only; Harness never installs
// or runs it). Run pictures go to the active project's outputs, or else to
// `<data dir>/outputs/comfyui`.

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
    /// The project output it was recorded as (open and reveal go through the project then).
    output_id: Option<String>,
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
/// the pictures are saved (into `project`'s outputs when given), or fails with
/// `comfyui.cancelled` after [`comfyui_cancel`].
#[tauri::command]
pub(crate) async fn comfyui_run(
    app: tauri::AppHandle,
    store: State<'_, ConversationStore>,
    source: comfy::WorkflowSource,
    params: comfy::Txt2ImgParams,
    project: Option<String>,
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
    let saved: Vec<(String, String, Option<String>)> = match project {
        Some(project_id) => {
            let db = store.0.clone()?;
            let images = images.clone();
            let from = harness_core::projects::OutputSource {
                source: "image".into(),
                model: Some(params.checkpoint.clone().unwrap_or_else(|| "ComfyUI".into())),
                conversation_id: None,
                agent_task_id: None,
            };
            blocking(move || {
                images
                    .iter()
                    .map(|i| {
                        let o = harness_core::projects::save_output(&db, &project_id, &i.filename, &i.bytes, &from)?;
                        Ok((o.path, comfy::image_format(&i.bytes).unwrap_or("png").to_string(), Some(o.id)))
                    })
                    .collect::<Result<Vec<_>, String>>()
            })
            .await??
        }
        None => {
            let images = images.clone();
            blocking(move || comfy::save_images(&comfy::outputs_dir(), &images))
                .await??
                .into_iter()
                .map(|s| (s.path, s.format, None))
                .collect()
        }
    };
    Ok(ComfyRunResult {
        prompt_id: outcome.prompt_id,
        seed,
        images: saved
            .into_iter()
            .zip(images)
            .map(|((path, format, output_id), i)| ComfyRunImage {
                path,
                format,
                output_id,
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
