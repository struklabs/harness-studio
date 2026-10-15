from typing import Literal

FailureKind = Literal[
    "corrupt_model",
    "crashed",
    "device_lost",
    "draft_load_failed",
    "invalid_argument",
    "load_timeout",
    "missing_library",
    "model_not_found",
    "out_of_memory",
    "port_in_use",
    "start_failed",
    "unsupported_architecture",
]

FAILURE_KIND_VALUES: set[FailureKind] = {
    "corrupt_model",
    "crashed",
    "device_lost",
    "draft_load_failed",
    "invalid_argument",
    "load_timeout",
    "missing_library",
    "model_not_found",
    "out_of_memory",
    "port_in_use",
    "start_failed",
    "unsupported_architecture",
}


def check_failure_kind(value: str) -> FailureKind:
    if value in FAILURE_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {FAILURE_KIND_VALUES!r}")
