from typing import Literal

ConvertStage = Literal[
    "converting", "done", "downloading", "finalizing", "preparing", "quantizing", "queued"
]

CONVERT_STAGE_VALUES: set[ConvertStage] = {
    "converting",
    "done",
    "downloading",
    "finalizing",
    "preparing",
    "quantizing",
    "queued",
}


def check_convert_stage(value: str) -> ConvertStage:
    if value in CONVERT_STAGE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {CONVERT_STAGE_VALUES!r}")
