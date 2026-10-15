from typing import Literal

McpAuthState = Literal["authorized", "required"]

MCP_AUTH_STATE_VALUES: set[McpAuthState] = {
    "authorized",
    "required",
}


def check_mcp_auth_state(value: str) -> McpAuthState:
    if value in MCP_AUTH_STATE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MCP_AUTH_STATE_VALUES!r}")
