from typing import Literal

RuntimeTrust = Literal["local_files", "user_confirmed", "verified"]

RUNTIME_TRUST_VALUES: set[RuntimeTrust] = {
    "local_files",
    "user_confirmed",
    "verified",
}


def check_runtime_trust(value: str) -> RuntimeTrust:
    if value in RUNTIME_TRUST_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {RUNTIME_TRUST_VALUES!r}")
