from typing import Literal

PermissionClass = Literal["dangerous", "restricted", "safe"]

PERMISSION_CLASS_VALUES: set[PermissionClass] = {
    "dangerous",
    "restricted",
    "safe",
}


def check_permission_class(value: str) -> PermissionClass:
    if value in PERMISSION_CLASS_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {PERMISSION_CLASS_VALUES!r}")
