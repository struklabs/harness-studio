from typing import Literal

Decision = Literal["ask", "refuse", "run"]

DECISION_VALUES: set[Decision] = {
    "ask",
    "refuse",
    "run",
}


def check_decision(value: str) -> Decision:
    if value in DECISION_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {DECISION_VALUES!r}")
