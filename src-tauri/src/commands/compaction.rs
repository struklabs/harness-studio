// Playground context compaction (`harness_core::compaction`): summarize the older turns of
// a conversation into a marker, undo / re-apply it.

/// Compacts the conversation's current path (summary request through `request.endpoint`).
/// One compaction per conversation at a time (`compaction.busy`).
#[tauri::command]
pub(crate) async fn compact_conversation(
    store: State<'_, ConversationStore>,
    request: harness_core::compaction::CompactRequest,
) -> Result<harness_core::compaction::CompactOutcome, String> {
    let db = store.0.clone()?;
    let jobs = harness_core::compaction::jobs();
    let flag = jobs
        .register(&request.conversation_id)
        .ok_or_else(|| String::from(harness_core::error_key::err("compaction.busy")))?;
    let out = harness_core::compaction::compact(&db, &request, &flag).await;
    jobs.finish(&request.conversation_id, &flag);
    out
}

/// Stops a running compaction of the conversation; false when none runs.
#[tauri::command]
pub(crate) fn cancel_compaction(conversation_id: String) -> bool {
    harness_core::compaction::jobs().cancel(&conversation_id)
}

/// Undoes (`undone = true`) or re-applies a compaction marker.
#[tauri::command]
pub(crate) async fn set_compaction_undone(
    store: State<'_, ConversationStore>,
    message_id: String,
    undone: bool,
) -> Result<ConversationView, String> {
    let db = store.0.clone()?;
    blocking(move || harness_core::compaction::set_undone(&db, &message_id, undone)).await?
}

/// Counts the rendered next chat request, including its server chat template and tools.
#[tauri::command]
pub(crate) async fn count_playground_tokens(
    tokenize: Option<harness_core::compaction::TokenizeEndpoint>,
    body: serde_json::Value,
) -> Result<(u64, bool), String> {
    harness_core::compaction::count_request(tokenize.as_ref(), &body).await
}
