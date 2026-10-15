#[tauri::command]
pub(crate) fn link_list() -> Vec<harness_core::link::LinkView> {
    harness_core::link::list()
}

#[tauri::command]
pub(crate) fn link_models() -> Vec<harness_core::link::ModelView> {
    harness_core::link::models()
}

#[tauri::command]
pub(crate) async fn link_add(
    name: String,
    base_url: String,
    token: String,
    allow_insecure_http: bool,
) -> Result<harness_core::link::LinkView, String> {
    harness_core::link::add(name, base_url, token, allow_insecure_http).await
}

#[tauri::command]
pub(crate) async fn link_update(
    id: String,
    name: String,
    base_url: String,
    token: Option<String>,
    allow_insecure_http: bool,
    enabled: bool,
) -> Result<harness_core::link::LinkView, String> {
    harness_core::link::update(id, name, base_url, token, allow_insecure_http, enabled).await
}

#[tauri::command]
pub(crate) async fn link_remove(id: String) -> Result<Option<String>, String> {
    harness_core::link::remove(&id).await
}

#[tauri::command]
pub(crate) async fn link_test(id: String) -> Result<harness_core::link::LinkStatus, String> {
    harness_core::link::test(&id).await
}
