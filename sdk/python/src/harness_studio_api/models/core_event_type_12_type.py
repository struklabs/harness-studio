from typing import Literal

CoreEventType12Type = Literal["gateway_model"]

CORE_EVENT_TYPE_12_TYPE_VALUES: set[CoreEventType12Type] = {
    "gateway_model",
}


def check_core_event_type_12_type(value: str) -> CoreEventType12Type:
    if value in CORE_EVENT_TYPE_12_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_12_TYPE_VALUES!r}"
    )
