from typing import Literal

CoreEventType9Type = Literal["server_failed"]

CORE_EVENT_TYPE_9_TYPE_VALUES: set[CoreEventType9Type] = {
    "server_failed",
}


def check_core_event_type_9_type(value: str) -> CoreEventType9Type:
    if value in CORE_EVENT_TYPE_9_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_9_TYPE_VALUES!r}"
    )
