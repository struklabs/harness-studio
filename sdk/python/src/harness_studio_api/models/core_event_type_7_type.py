from typing import Literal

CoreEventType7Type = Literal["benchmark_progress"]

CORE_EVENT_TYPE_7_TYPE_VALUES: set[CoreEventType7Type] = {
    "benchmark_progress",
}


def check_core_event_type_7_type(value: str) -> CoreEventType7Type:
    if value in CORE_EVENT_TYPE_7_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_7_TYPE_VALUES!r}"
    )
