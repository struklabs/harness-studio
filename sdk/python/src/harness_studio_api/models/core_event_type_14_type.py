from typing import Literal

CoreEventType14Type = Literal["analytics_written"]

CORE_EVENT_TYPE_14_TYPE_VALUES: set[CoreEventType14Type] = {
    "analytics_written",
}


def check_core_event_type_14_type(value: str) -> CoreEventType14Type:
    if value in CORE_EVENT_TYPE_14_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_14_TYPE_VALUES!r}"
    )
