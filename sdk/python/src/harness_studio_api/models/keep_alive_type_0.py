from typing import Literal

KeepAliveType0 = Literal["forever"]

KEEP_ALIVE_TYPE_0_VALUES: set[KeepAliveType0] = {
    "forever",
}


def check_keep_alive_type_0(value: str) -> KeepAliveType0:
    if value in KEEP_ALIVE_TYPE_0_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {KEEP_ALIVE_TYPE_0_VALUES!r}")
