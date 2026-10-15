from typing import Literal

Support = Literal["supported", "unknown", "unsupported"]

SUPPORT_VALUES: set[Support] = {
    "supported",
    "unknown",
    "unsupported",
}


def check_support(value: str) -> Support:
    if value in SUPPORT_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SUPPORT_VALUES!r}")
