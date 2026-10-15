from typing import Literal

CoreEventType2Type = Literal["server_status"]

CORE_EVENT_TYPE_2_TYPE_VALUES: set[CoreEventType2Type] = {
    "server_status",
}


def check_core_event_type_2_type(value: str) -> CoreEventType2Type:
    if value in CORE_EVENT_TYPE_2_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_2_TYPE_VALUES!r}"
    )
