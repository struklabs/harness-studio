from typing import Literal

CoreEventType13Type = Literal["last_session_saved"]

CORE_EVENT_TYPE_13_TYPE_VALUES: set[CoreEventType13Type] = {
    "last_session_saved",
}


def check_core_event_type_13_type(value: str) -> CoreEventType13Type:
    if value in CORE_EVENT_TYPE_13_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_13_TYPE_VALUES!r}"
    )
