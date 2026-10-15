from typing import Literal

CoreEventType3Type = Literal["download"]

CORE_EVENT_TYPE_3_TYPE_VALUES: set[CoreEventType3Type] = {
    "download",
}


def check_core_event_type_3_type(value: str) -> CoreEventType3Type:
    if value in CORE_EVENT_TYPE_3_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_3_TYPE_VALUES!r}"
    )
