from typing import Literal

CoreEventType10Type = Literal["server_restart"]

CORE_EVENT_TYPE_10_TYPE_VALUES: set[CoreEventType10Type] = {
    "server_restart",
}


def check_core_event_type_10_type(value: str) -> CoreEventType10Type:
    if value in CORE_EVENT_TYPE_10_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_10_TYPE_VALUES!r}"
    )
