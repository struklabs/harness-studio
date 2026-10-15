from typing import Literal

Scope = Literal[
    "admin",
    "conversations",
    "downloads:write",
    "inference",
    "models:write",
    "read",
    "servers:control",
    "servers:write",
    "tools",
]

SCOPE_VALUES: set[Scope] = {
    "admin",
    "conversations",
    "downloads:write",
    "inference",
    "models:write",
    "read",
    "servers:control",
    "servers:write",
    "tools",
}


def check_scope(value: str) -> Scope:
    if value in SCOPE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SCOPE_VALUES!r}")
