from typing import Literal

CoreEventType1Type = Literal["tools_changed"]

CORE_EVENT_TYPE_1_TYPE_VALUES: set[CoreEventType1Type] = {
    "tools_changed",
}


def check_core_event_type_1_type(value: str) -> CoreEventType1Type:
    if value in CORE_EVENT_TYPE_1_TYPE_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {CORE_EVENT_TYPE_1_TYPE_VALUES!r}"
    )
