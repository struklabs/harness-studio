from typing import Literal

ToolPolicy = Literal["allow_all", "allow_trusted", "ask_every_time", "off"]

TOOL_POLICY_VALUES: set[ToolPolicy] = {
    "allow_all",
    "allow_trusted",
    "ask_every_time",
    "off",
}


def check_tool_policy(value: str) -> ToolPolicy:
    if value in TOOL_POLICY_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {TOOL_POLICY_VALUES!r}")
