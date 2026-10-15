from typing import Literal

KvLevel = Literal["balanced", "compact", "custom", "full", "keys_only", "saver"]

KV_LEVEL_VALUES: set[KvLevel] = {
    "balanced",
    "compact",
    "custom",
    "full",
    "keys_only",
    "saver",
}


def check_kv_level(value: str) -> KvLevel:
    if value in KV_LEVEL_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {KV_LEVEL_VALUES!r}")
