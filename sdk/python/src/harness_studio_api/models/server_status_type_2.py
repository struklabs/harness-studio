from typing import Literal

ServerStatusType2 = Literal["Running"]

SERVER_STATUS_TYPE_2_VALUES: set[ServerStatusType2] = {
    "Running",
}


def check_server_status_type_2(value: str) -> ServerStatusType2:
    if value in SERVER_STATUS_TYPE_2_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SERVER_STATUS_TYPE_2_VALUES!r}")
