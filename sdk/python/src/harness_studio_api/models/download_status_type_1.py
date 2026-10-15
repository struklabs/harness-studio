from typing import Literal

DownloadStatusType1 = Literal["Queued"]

DOWNLOAD_STATUS_TYPE_1_VALUES: set[DownloadStatusType1] = {
    "Queued",
}


def check_download_status_type_1(value: str) -> DownloadStatusType1:
    if value in DOWNLOAD_STATUS_TYPE_1_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {DOWNLOAD_STATUS_TYPE_1_VALUES!r}"
    )
