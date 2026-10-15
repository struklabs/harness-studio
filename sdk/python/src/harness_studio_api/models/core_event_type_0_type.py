from typing import Literal

CoreEventType0Type = Literal["definitions_changed"]

CORE_EVENT_TYPE_0_TYPE_VALUES: set[CoreEventType0Type] = {
    "definitions_changed",
}


def check_core_event_type_0_type(value: str) -> CoreEventType0Type:
    if value in CORE_EVENT_TYPE_0_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_0_TYPE_VALUES!r}"
    )
