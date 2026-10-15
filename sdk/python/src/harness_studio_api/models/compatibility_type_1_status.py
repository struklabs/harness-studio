from typing import Literal

CompatibilityType1Status = Literal["needs_runtime"]

COMPATIBILITY_TYPE_1_STATUS_VALUES: set[CompatibilityType1Status] = {
    "needs_runtime",
}


def check_compatibility_type_1_status(value: str) -> CompatibilityType1Status:
    if value in COMPATIBILITY_TYPE_1_STATUS_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {COMPATIBILITY_TYPE_1_STATUS_VALUES!r}"
    )
