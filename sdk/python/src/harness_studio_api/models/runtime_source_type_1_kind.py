from typing import Literal

RuntimeSourceType1Kind = Literal["local"]

RUNTIME_SOURCE_TYPE_1_KIND_VALUES: set[RuntimeSourceType1Kind] = {
    "local",
}


def check_runtime_source_type_1_kind(value: str) -> RuntimeSourceType1Kind:
    if value in RUNTIME_SOURCE_TYPE_1_KIND_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {RUNTIME_SOURCE_TYPE_1_KIND_VALUES!r}"
    )
