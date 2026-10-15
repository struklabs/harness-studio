from typing import Literal

SlotOrigin = Literal["agents", "gateway", "user"]

SLOT_ORIGIN_VALUES: set[SlotOrigin] = {
    "agents",
    "gateway",
    "user",
}


def check_slot_origin(value: str) -> SlotOrigin:
    if value in SLOT_ORIGIN_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SLOT_ORIGIN_VALUES!r}")
