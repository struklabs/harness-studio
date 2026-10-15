from typing import Literal

DownloadStatusType0 = Literal["Idle"]

DOWNLOAD_STATUS_TYPE_0_VALUES: set[DownloadStatusType0] = {
    "Idle",
}


def check_download_status_type_0(value: str) -> DownloadStatusType0:
    if value in DOWNLOAD_STATUS_TYPE_0_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {DOWNLOAD_STATUS_TYPE_0_VALUES!r}"
    )
