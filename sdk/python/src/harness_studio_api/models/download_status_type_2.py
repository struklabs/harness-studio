from typing import Literal

DownloadStatusType2 = Literal["Downloading"]

DOWNLOAD_STATUS_TYPE_2_VALUES: set[DownloadStatusType2] = {
    "Downloading",
}


def check_download_status_type_2(value: str) -> DownloadStatusType2:
    if value in DOWNLOAD_STATUS_TYPE_2_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {DOWNLOAD_STATUS_TYPE_2_VALUES!r}"
    )
