from typing import Literal

CompactionTrigger = Literal["auto", "manual"]

COMPACTION_TRIGGER_VALUES: set[CompactionTrigger] = {
    "auto",
    "manual",
}


def check_compaction_trigger(value: str) -> CompactionTrigger:
    if value in COMPACTION_TRIGGER_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {COMPACTION_TRIGGER_VALUES!r}")
