from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.download_status_type_0 import (
    DownloadStatusType0,
    check_download_status_type_0,
)
from ..models.download_status_type_1 import (
    DownloadStatusType1,
    check_download_status_type_1,
)
from ..models.download_status_type_2 import (
    DownloadStatusType2,
    check_download_status_type_2,
)
from ..models.download_status_type_3 import (
    DownloadStatusType3,
    check_download_status_type_3,
)
from ..models.download_status_type_4 import (
    DownloadStatusType4,
    check_download_status_type_4,
)
from ..models.download_status_type_5 import (
    DownloadStatusType5,
    check_download_status_type_5,
)
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.download_status_type_6 import DownloadStatusType6


T = TypeVar("T", bound="DownloadProgress")


@_attrs_define
class DownloadProgress:
    """
    Attributes:
        repo_id (str):
        variant_name (str):
        total_bytes (int):
        downloaded_bytes (int):
        current_shard_name (str):
        current_shard_index (int):
        total_shards (int):
        speed_bytes_per_sec (int):
        status (DownloadStatusType0 | DownloadStatusType1 | DownloadStatusType2 | DownloadStatusType3 |
            DownloadStatusType4 | DownloadStatusType5 | DownloadStatusType6):
        id (int | Unset): Queue job id.
        created_at (int | Unset):
    """

    repo_id: str
    variant_name: str
    total_bytes: int
    downloaded_bytes: int
    current_shard_name: str
    current_shard_index: int
    total_shards: int
    speed_bytes_per_sec: int
    status: (
        DownloadStatusType0
        | DownloadStatusType1
        | DownloadStatusType2
        | DownloadStatusType3
        | DownloadStatusType4
        | DownloadStatusType5
        | DownloadStatusType6
    )
    id: int | Unset = UNSET
    created_at: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo_id = self.repo_id

        variant_name = self.variant_name

        total_bytes = self.total_bytes

        downloaded_bytes = self.downloaded_bytes

        current_shard_name = self.current_shard_name

        current_shard_index = self.current_shard_index

        total_shards = self.total_shards

        speed_bytes_per_sec = self.speed_bytes_per_sec

        status: dict[str, Any] | str
        if isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        else:
            status = self.status.to_dict()

        id = self.id

        created_at = self.created_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo_id": repo_id,
                "variant_name": variant_name,
                "total_bytes": total_bytes,
                "downloaded_bytes": downloaded_bytes,
                "current_shard_name": current_shard_name,
                "current_shard_index": current_shard_index,
                "total_shards": total_shards,
                "speed_bytes_per_sec": speed_bytes_per_sec,
                "status": status,
            }
        )
        if id is not UNSET:
            field_dict["id"] = id
        if created_at is not UNSET:
            field_dict["created_at"] = created_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.download_status_type_6 import DownloadStatusType6  # noqa: PLC0415

        d = dict(src_dict)
        repo_id = d.pop("repo_id")

        variant_name = d.pop("variant_name")

        total_bytes = d.pop("total_bytes")

        downloaded_bytes = d.pop("downloaded_bytes")

        current_shard_name = d.pop("current_shard_name")

        current_shard_index = d.pop("current_shard_index")

        total_shards = d.pop("total_shards")

        speed_bytes_per_sec = d.pop("speed_bytes_per_sec")

        def _parse_status(
            data: object,
        ) -> (
            DownloadStatusType0
            | DownloadStatusType1
            | DownloadStatusType2
            | DownloadStatusType3
            | DownloadStatusType4
            | DownloadStatusType5
            | DownloadStatusType6
        ):
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_download_status_type_0 = check_download_status_type_0(data)

                return componentsschemas_download_status_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_download_status_type_1 = check_download_status_type_1(data)

                return componentsschemas_download_status_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_download_status_type_2 = check_download_status_type_2(data)

                return componentsschemas_download_status_type_2
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_download_status_type_3 = check_download_status_type_3(data)

                return componentsschemas_download_status_type_3
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_download_status_type_4 = check_download_status_type_4(data)

                return componentsschemas_download_status_type_4
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_download_status_type_5 = check_download_status_type_5(data)

                return componentsschemas_download_status_type_5
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_download_status_type_6 = DownloadStatusType6.from_dict(data)

            return componentsschemas_download_status_type_6

        status = _parse_status(d.pop("status"))

        id = d.pop("id", UNSET)

        created_at = d.pop("created_at", UNSET)

        download_progress = cls(
            repo_id=repo_id,
            variant_name=variant_name,
            total_bytes=total_bytes,
            downloaded_bytes=downloaded_bytes,
            current_shard_name=current_shard_name,
            current_shard_index=current_shard_index,
            total_shards=total_shards,
            speed_bytes_per_sec=speed_bytes_per_sec,
            status=status,
            id=id,
            created_at=created_at,
        )

        download_progress.additional_properties = d
        return download_progress

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
