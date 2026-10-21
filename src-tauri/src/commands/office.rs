// Thin desktop adapters for the same office store used by the Control API.
#[tauri::command]
pub(crate) async fn office_list(presets:bool)->Result<Vec<harness_core::office::OfficeScene>,String> {
    blocking(move ||harness_core::office::list(&harness_core::paths::data_root(),presets)).await?
}
#[tauri::command]
pub(crate) async fn office_save(presets:bool,scene:harness_core::office::OfficeScene)->Result<harness_core::office::OfficeScene,String> {
    blocking(move ||harness_core::office::save(&harness_core::paths::data_root(),presets,scene)).await?
}
#[tauri::command]
pub(crate) async fn office_delete(presets:bool,id:String)->Result<bool,String> {
    blocking(move ||harness_core::office::remove(&harness_core::paths::data_root(),presets,&id)).await?
}
#[tauri::command]
pub(crate) async fn office_activity()->Result<Vec<harness_core::office::OfficeCharacter>,String> {
    blocking(move ||harness_core::office::activity(&harness_core::paths::data_root())).await?
}
#[tauri::command]
pub(crate) async fn office_run(id:String)->Result<Option<serde_json::Value>,String> {
    blocking(move ||harness_core::office::run(&harness_core::paths::data_root(),&id)).await?
}
