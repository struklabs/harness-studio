from typing import Literal

FitCategory = Literal["cpu_heavy", "full_gpu", "hybrid", "oom_risk", "tight"]

FIT_CATEGORY_VALUES: set[FitCategory] = {
    "cpu_heavy",
    "full_gpu",
    "hybrid",
    "oom_risk",
    "tight",
}


def check_fit_category(value: str) -> FitCategory:
    if value in FIT_CATEGORY_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {FIT_CATEGORY_VALUES!r}")
