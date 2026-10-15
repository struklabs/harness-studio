from typing import Literal

CompatibilityType3Status = Literal["unsupported"]

COMPATIBILITY_TYPE_3_STATUS_VALUES: set[CompatibilityType3Status] = {
    "unsupported",
}


def check_compatibility_type_3_status(value: str) -> CompatibilityType3Status:
    if value in COMPATIBILITY_TYPE_3_STATUS_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {COMPATIBILITY_TYPE_3_STATUS_VALUES!r}"
    )
