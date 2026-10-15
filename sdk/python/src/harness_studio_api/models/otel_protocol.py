from typing import Literal

OtelProtocol = Literal["grpc", "http_protobuf"]

OTEL_PROTOCOL_VALUES: set[OtelProtocol] = {
    "grpc",
    "http_protobuf",
}


def check_otel_protocol(value: str) -> OtelProtocol:
    if value in OTEL_PROTOCOL_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {OTEL_PROTOCOL_VALUES!r}")
