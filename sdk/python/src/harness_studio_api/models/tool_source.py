from typing import Literal

ToolSource = Literal["builtin", "generated", "installed", "mcp"]

TOOL_SOURCE_VALUES: set[ToolSource] = {
    "builtin",
    "generated",
    "installed",
    "mcp",
}


def check_tool_source(value: str) -> ToolSource:
    if value in TOOL_SOURCE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {TOOL_SOURCE_VALUES!r}")
