from typing import Literal

CoreEventType8Type = Literal["benchmark_finished"]

CORE_EVENT_TYPE_8_TYPE_VALUES: set[CoreEventType8Type] = {
    "benchmark_finished",
}


def check_core_event_type_8_type(value: str) -> CoreEventType8Type:
    if value in CORE_EVENT_TYPE_8_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_8_TYPE_VALUES!r}"
    )
