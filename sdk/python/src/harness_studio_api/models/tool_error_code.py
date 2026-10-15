from typing import Literal

ToolErrorCode = Literal[
    "approval_required",
    "cancelled",
    "failed",
    "invalid_arguments",
    "io",
    "network",
    "no_workspace",
    "not_configured",
    "not_found",
    "outside_workspace",
    "policy_off",
    "response_too_large",
    "timeout",
    "unknown_tool",
    "unsupported_content_type",
]

TOOL_ERROR_CODE_VALUES: set[ToolErrorCode] = {
    "approval_required",
    "cancelled",
    "failed",
    "invalid_arguments",
    "io",
    "network",
    "no_workspace",
    "not_configured",
    "not_found",
    "outside_workspace",
    "policy_off",
    "response_too_large",
    "timeout",
    "unknown_tool",
    "unsupported_content_type",
}


def check_tool_error_code(value: str) -> ToolErrorCode:
    if value in TOOL_ERROR_CODE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {TOOL_ERROR_CODE_VALUES!r}")
