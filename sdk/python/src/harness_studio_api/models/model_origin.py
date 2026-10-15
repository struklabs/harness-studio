from typing import Literal

ModelOrigin = Literal["library", "linked"]

MODEL_ORIGIN_VALUES: set[ModelOrigin] = {
    "library",
    "linked",
}


def check_model_origin(value: str) -> ModelOrigin:
    if value in MODEL_ORIGIN_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MODEL_ORIGIN_VALUES!r}")
