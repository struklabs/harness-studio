from typing import Literal

CompatibilityType0Status = Literal["runnable"]

COMPATIBILITY_TYPE_0_STATUS_VALUES: set[CompatibilityType0Status] = {
    "runnable",
}


def check_compatibility_type_0_status(value: str) -> CompatibilityType0Status:
    if value in COMPATIBILITY_TYPE_0_STATUS_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {COMPATIBILITY_TYPE_0_STATUS_VALUES!r}"
    )
