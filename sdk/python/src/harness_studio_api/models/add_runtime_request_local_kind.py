from typing import Literal

AddRuntimeRequestLocalKind = Literal["local"]

ADD_RUNTIME_REQUEST_LOCAL_KIND_VALUES: set[AddRuntimeRequestLocalKind] = {
    "local",
}


def check_add_runtime_request_local_kind(value: str) -> AddRuntimeRequestLocalKind:
    if value in ADD_RUNTIME_REQUEST_LOCAL_KIND_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {ADD_RUNTIME_REQUEST_LOCAL_KIND_VALUES!r}"
    )
