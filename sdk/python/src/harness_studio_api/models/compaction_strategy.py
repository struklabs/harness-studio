from typing import Literal

CompactionStrategy = Literal["brief", "incremental", "structured"]

COMPACTION_STRATEGY_VALUES: set[CompactionStrategy] = {
    "brief",
    "incremental",
    "structured",
}


def check_compaction_strategy(value: str) -> CompactionStrategy:
    if value in COMPACTION_STRATEGY_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {COMPACTION_STRATEGY_VALUES!r}")
