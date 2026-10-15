from typing import Literal

BinarySource = Literal["custom", "managed", "system"]

BINARY_SOURCE_VALUES: set[BinarySource] = {
    "custom",
    "managed",
    "system",
}


def check_binary_source(value: str) -> BinarySource:
    if value in BINARY_SOURCE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {BINARY_SOURCE_VALUES!r}")
