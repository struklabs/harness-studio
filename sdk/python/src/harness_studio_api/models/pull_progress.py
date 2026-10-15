from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="PullProgress")


@_attrs_define
class PullProgress:
    """Progress of a model pull or a backend install (the SSE `progress` events of lemond).

    Attributes:
        target (str): What is being fetched (model name or `recipe:backend`).
        file (str): File being downloaded.
        file_index (int):
        total_files (int):
        bytes_downloaded (int):
        bytes_total (int):
        total_download_size (int): Size of the whole pull when lemond knows it (0 = unknown).
        percent (float):
    """

    target: str
    file: str
    file_index: int
    total_files: int
    bytes_downloaded: int
    bytes_total: int
    total_download_size: int
    percent: float
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        target = self.target

        file = self.file

        file_index = self.file_index

        total_files = self.total_files

        bytes_downloaded = self.bytes_downloaded

        bytes_total = self.bytes_total

        total_download_size = self.total_download_size

        percent = self.percent

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "target": target,
                "file": file,
                "file_index": file_index,
                "total_files": total_files,
                "bytes_downloaded": bytes_downloaded,
                "bytes_total": bytes_total,
                "total_download_size": total_download_size,
                "percent": percent,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        target = d.pop("target")

        file = d.pop("file")

        file_index = d.pop("file_index")

        total_files = d.pop("total_files")

        bytes_downloaded = d.pop("bytes_downloaded")

        bytes_total = d.pop("bytes_total")

        total_download_size = d.pop("total_download_size")

        percent = d.pop("percent")

        pull_progress = cls(
            target=target,
            file=file,
            file_index=file_index,
            total_files=total_files,
            bytes_downloaded=bytes_downloaded,
            bytes_total=bytes_total,
            total_download_size=total_download_size,
            percent=percent,
        )

        pull_progress.additional_properties = d
        return pull_progress

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
