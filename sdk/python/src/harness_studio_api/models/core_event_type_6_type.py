from typing import Literal

CoreEventType6Type = Literal["convert_removed"]

CORE_EVENT_TYPE_6_TYPE_VALUES: set[CoreEventType6Type] = {
    "convert_removed",
}


def check_core_event_type_6_type(value: str) -> CoreEventType6Type:
    if value in CORE_EVENT_TYPE_6_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_6_TYPE_VALUES!r}"
    )
