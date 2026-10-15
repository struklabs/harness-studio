from typing import Literal

RecoveryKind = Literal[
    "cpu_moe",
    "enable_fit",
    "kv_cache_quant",
    "lower_context",
    "lower_gpu_layers",
    "lower_ubatch",
    "unload_server",
]

RECOVERY_KIND_VALUES: set[RecoveryKind] = {
    "cpu_moe",
    "enable_fit",
    "kv_cache_quant",
    "lower_context",
    "lower_gpu_layers",
    "lower_ubatch",
    "unload_server",
}


def check_recovery_kind(value: str) -> RecoveryKind:
    if value in RECOVERY_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {RECOVERY_KIND_VALUES!r}")
