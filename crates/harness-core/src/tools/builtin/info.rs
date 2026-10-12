//! `get_info`: the operating system and the working directory the tools are confined to.

use serde_json::{json, Value};

use crate::tools::registry::ToolCtx;
use crate::tools::types::*;

pub fn get_info(_a: &Value, ctx: &ToolCtx) -> Result<ToolOutput, ToolError> {
    let ws = ctx.workspace()?;
    let os = sysinfo::System::long_os_version()
        .or_else(sysinfo::System::name)
        .unwrap_or_else(|| "unknown".to_string());
    let text = serde_json::to_string_pretty(&json!({
        "os": os,
        "cwd": ws.display_root(),
    }))
    .unwrap_or_default();
    Ok(ToolOutput::text(text))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{run_in, tree};
    use crate::tools::ToolErrorCode;
    use serde_json::json;

    #[tokio::test]
    async fn reports_the_os_and_the_working_directory() {
        let d = tree(&[]);
        let r = run_in(d.path(), "get_info", json!({})).await;
        assert!(!r.is_error(), "{}", r.content);
        let v: serde_json::Value = serde_json::from_str(&r.content).unwrap();
        assert!(!v["os"].as_str().unwrap().is_empty());
        let cwd = std::fs::canonicalize(d.path()).unwrap();
        assert_eq!(
            std::path::Path::new(v["cwd"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            cwd
        );
    }

    #[tokio::test]
    async fn without_a_working_directory_it_says_so() {
        use crate::tools::registry::{RunLimits, ToolCtx, ToolRegistry};
        use crate::tools::{ToolInvocation, ToolOrigin};
        let reg = ToolRegistry::with_builtin();
        let r = reg
            .run(
                &ToolInvocation::new("get_info", json!({}), ToolOrigin::new("t")),
                &ToolCtx::new(None),
                &RunLimits::default(),
            )
            .await;
        assert_eq!(r.error, Some(ToolErrorCode::NoWorkspace));
    }
}
