from typing import Literal

DownloadStatusType5 = Literal["Completed"]

DOWNLOAD_STATUS_TYPE_5_VALUES: set[DownloadStatusType5] = {
    "Completed",
}


def check_download_status_type_5(value: str) -> DownloadStatusType5:
    if value in DOWNLOAD_STATUS_TYPE_5_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {DOWNLOAD_STATUS_TYPE_5_VALUES!r}"
    )
