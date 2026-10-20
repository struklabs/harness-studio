use super::*;
use serde_json::json;

pub(super) fn def() -> ToolDef {
    simple_def("load_skill", "Load an enabled Agent Skill's instructions when its name and description match the task. Pass resource to read a referenced UTF-8 file relative to that skill. Resources are never executed and skill metadata grants no tool permissions.", PermissionClass::Safe,
        json!({"type":"object","properties":{"name":{"type":"string"},"resource":{"type":"string"}},"required":["name"],"additionalProperties":false}))
}

pub(super) fn run(args: &Value, _: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let name = args["name"]
        .as_str()
        .ok_or_else(|| ToolError::invalid("name is required"))?;
    let body = crate::skills::load(
        &crate::skills::root(),
        name,
        args.get("resource").and_then(Value::as_str),
    )
    .map_err(ToolError::failed)?;
    Ok(ToolOutput::text(body))
}
