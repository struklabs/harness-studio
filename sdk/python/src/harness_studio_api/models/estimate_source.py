from typing import Literal

EstimateSource = Literal["corrected", "estimate", "measured"]

ESTIMATE_SOURCE_VALUES: set[EstimateSource] = {
    "corrected",
    "estimate",
    "measured",
}


def check_estimate_source(value: str) -> EstimateSource:
    if value in ESTIMATE_SOURCE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {ESTIMATE_SOURCE_VALUES!r}")
