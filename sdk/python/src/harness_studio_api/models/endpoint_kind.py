from typing import Literal

EndpointKind = Literal["direct", "managed"]

ENDPOINT_KIND_VALUES: set[EndpointKind] = {
    "direct",
    "managed",
}


def check_endpoint_kind(value: str) -> EndpointKind:
    if value in ENDPOINT_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {ENDPOINT_KIND_VALUES!r}")
