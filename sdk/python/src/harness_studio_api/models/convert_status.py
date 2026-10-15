from typing import Literal

ConvertStatus = Literal["cancelled", "completed", "failed", "queued", "running"]

CONVERT_STATUS_VALUES: set[ConvertStatus] = {
    "cancelled",
    "completed",
    "failed",
    "queued",
    "running",
}


def check_convert_status(value: str) -> ConvertStatus:
    if value in CONVERT_STATUS_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {CONVERT_STATUS_VALUES!r}")
