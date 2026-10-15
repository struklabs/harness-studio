from typing import Literal

McpServerState = Literal["connected", "connecting", "disabled", "failed", "idle"]

MCP_SERVER_STATE_VALUES: set[McpServerState] = {
    "connected",
    "connecting",
    "disabled",
    "failed",
    "idle",
}


def check_mcp_server_state(value: str) -> McpServerState:
    if value in MCP_SERVER_STATE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MCP_SERVER_STATE_VALUES!r}")
