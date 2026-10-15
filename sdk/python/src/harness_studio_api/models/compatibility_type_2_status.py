from typing import Literal

CompatibilityType2Status = Literal["unsupported_platform"]

COMPATIBILITY_TYPE_2_STATUS_VALUES: set[CompatibilityType2Status] = {
    "unsupported_platform",
}


def check_compatibility_type_2_status(value: str) -> CompatibilityType2Status:
    if value in COMPATIBILITY_TYPE_2_STATUS_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {COMPATIBILITY_TYPE_2_STATUS_VALUES!r}"
    )
