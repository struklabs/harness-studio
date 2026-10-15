from typing import Literal

DownloadStatusType4 = Literal["Paused"]

DOWNLOAD_STATUS_TYPE_4_VALUES: set[DownloadStatusType4] = {
    "Paused",
}


def check_download_status_type_4(value: str) -> DownloadStatusType4:
    if value in DOWNLOAD_STATUS_TYPE_4_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {DOWNLOAD_STATUS_TYPE_4_VALUES!r}"
    )
