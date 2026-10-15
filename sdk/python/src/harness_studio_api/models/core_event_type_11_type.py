from typing import Literal

CoreEventType11Type = Literal["gateway_request"]

CORE_EVENT_TYPE_11_TYPE_VALUES: set[CoreEventType11Type] = {
    "gateway_request",
}


def check_core_event_type_11_type(value: str) -> CoreEventType11Type:
    if value in CORE_EVENT_TYPE_11_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_11_TYPE_VALUES!r}"
    )
