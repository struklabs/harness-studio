from typing import Literal

RouterModelState = Literal["failed", "loaded", "loading", "unloaded"]

ROUTER_MODEL_STATE_VALUES: set[RouterModelState] = {
    "failed",
    "loaded",
    "loading",
    "unloaded",
}


def check_router_model_state(value: str) -> RouterModelState:
    if value in ROUTER_MODEL_STATE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {ROUTER_MODEL_STATE_VALUES!r}")
