from typing import Literal

SlotState = Literal["draining", "loading", "ready", "unloaded"]

SLOT_STATE_VALUES: set[SlotState] = {
    "draining",
    "loading",
    "ready",
    "unloaded",
}


def check_slot_state(value: str) -> SlotState:
    if value in SLOT_STATE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SLOT_STATE_VALUES!r}")
