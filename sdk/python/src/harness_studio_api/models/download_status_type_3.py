from typing import Literal

DownloadStatusType3 = Literal["Verifying"]

DOWNLOAD_STATUS_TYPE_3_VALUES: set[DownloadStatusType3] = {
    "Verifying",
}


def check_download_status_type_3(value: str) -> DownloadStatusType3:
    if value in DOWNLOAD_STATUS_TYPE_3_VALUES:
        return value
    raise TypeError(
        f"Unexpected value {value!r}. Expected one of {DOWNLOAD_STATUS_TYPE_3_VALUES!r}"
    )
