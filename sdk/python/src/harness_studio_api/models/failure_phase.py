from typing import Literal

FailurePhase = Literal["loading", "running"]

FAILURE_PHASE_VALUES: set[FailurePhase] = {
    "loading",
    "running",
}


def check_failure_phase(value: str) -> FailurePhase:
    if value in FAILURE_PHASE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {FAILURE_PHASE_VALUES!r}")
