from typing import Literal

ServerStatusType1 = Literal["Starting"]

SERVER_STATUS_TYPE_1_VALUES: set[ServerStatusType1] = {
    "Starting",
}


def check_server_status_type_1(value: str) -> ServerStatusType1:
    if value in SERVER_STATUS_TYPE_1_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SERVER_STATUS_TYPE_1_VALUES!r}")
