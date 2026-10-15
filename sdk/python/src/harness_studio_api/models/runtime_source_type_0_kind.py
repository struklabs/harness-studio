from typing import Literal

RuntimeSourceType0Kind = Literal["github"]

RUNTIME_SOURCE_TYPE_0_KIND_VALUES: set[RuntimeSourceType0Kind] = {
    "github",
}


def check_runtime_source_type_0_kind(value: str) -> RuntimeSourceType0Kind:
    if value in RUNTIME_SOURCE_TYPE_0_KIND_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {RUNTIME_SOURCE_TYPE_0_KIND_VALUES!r}"
    )
