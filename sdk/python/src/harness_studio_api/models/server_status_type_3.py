from typing import Literal

ServerStatusType3 = Literal["Stopping"]

SERVER_STATUS_TYPE_3_VALUES: set[ServerStatusType3] = {
    "Stopping",
}


def check_server_status_type_3(value: str) -> ServerStatusType3:
    if value in SERVER_STATUS_TYPE_3_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SERVER_STATUS_TYPE_3_VALUES!r}")
