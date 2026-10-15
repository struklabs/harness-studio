from typing import Literal

CoreEventType5Type = Literal["convert"]

CORE_EVENT_TYPE_5_TYPE_VALUES: set[CoreEventType5Type] = {
    "convert",
}


def check_core_event_type_5_type(value: str) -> CoreEventType5Type:
    if value in CORE_EVENT_TYPE_5_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_5_TYPE_VALUES!r}"
    )
