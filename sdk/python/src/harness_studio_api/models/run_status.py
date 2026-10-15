from typing import Literal

RunStatus = Literal["cancelled", "completed", "error"]

RUN_STATUS_VALUES: set[RunStatus] = {
    "cancelled",
    "completed",
    "error",
}


def check_run_status(value: str) -> RunStatus:
    if value in RUN_STATUS_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {RUN_STATUS_VALUES!r}")
