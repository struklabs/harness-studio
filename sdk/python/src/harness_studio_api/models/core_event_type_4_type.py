from typing import Literal

CoreEventType4Type = Literal["download_removed"]

CORE_EVENT_TYPE_4_TYPE_VALUES: set[CoreEventType4Type] = {
    "download_removed",
}


def check_core_event_type_4_type(value: str) -> CoreEventType4Type:
    if value in CORE_EVENT_TYPE_4_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_4_TYPE_VALUES!r}"
    )
