from typing import Literal

KvReason = Literal["fits", "tightest", "to_fit", "too_big"]

KV_REASON_VALUES: set[KvReason] = {
    "fits",
    "tightest",
    "to_fit",
    "too_big",
}


def check_kv_reason(value: str) -> KvReason:
    if value in KV_REASON_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {KV_REASON_VALUES!r}")
