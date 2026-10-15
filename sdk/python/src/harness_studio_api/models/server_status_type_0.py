from typing import Literal

ServerStatusType0 = Literal["Stopped"]

SERVER_STATUS_TYPE_0_VALUES: set[ServerStatusType0] = {
    "Stopped",
}


def check_server_status_type_0(value: str) -> ServerStatusType0:
    if value in SERVER_STATUS_TYPE_0_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SERVER_STATUS_TYPE_0_VALUES!r}")
