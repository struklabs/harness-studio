from typing import Literal

InstanceKind = Literal["gui", "headless"]

INSTANCE_KIND_VALUES: set[InstanceKind] = {
    "gui",
    "headless",
}


def check_instance_kind(value: str) -> InstanceKind:
    if value in INSTANCE_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {INSTANCE_KIND_VALUES!r}")
