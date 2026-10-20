#[tauri::command]
pub(crate) fn cloud_list() -> Vec<harness_core::cloud::ProviderView> {
    harness_core::cloud::list()
}

#[tauri::command]
pub(crate) fn cloud_models() -> Vec<harness_core::cloud::ModelView> {
    harness_core::cloud::models()
}

#[tauri::command]
pub(crate) async fn cloud_add(
    input: harness_core::cloud::ProviderInput,
) -> Result<harness_core::cloud::ProviderView, String> {
    harness_core::cloud::add(input).await
}

#[tauri::command]
pub(crate) async fn cloud_update(
    id: String,
    input: harness_core::cloud::ProviderInput,
) -> Result<harness_core::cloud::ProviderView, String> {
    harness_core::cloud::update(id, input).await
}

#[tauri::command]
pub(crate) async fn cloud_remove(id: String) -> Result<Option<String>, String> {
    harness_core::cloud::remove(&id).await
}

#[tauri::command]
pub(crate) async fn cloud_test(id: String) -> Result<harness_core::cloud::ProviderStatus, String> {
    harness_core::cloud::test(&id).await
}

#[tauri::command]
pub(crate) async fn cloud_test_model(
    id: String,
    model: String,
) -> Result<harness_core::integrations::conntest::ConnectionTest, String> {
    harness_core::cloud::test_model(&id, &model).await
}
