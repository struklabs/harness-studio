from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="HfShardInfo")


@_attrs_define
class HfShardInfo:
    """
    Attributes:
        filename (str):
        download_url (str):
        size_bytes (int):
        shard_index (int):
        total_shards (int):
        sha256 (None | str | Unset): SHA-256 of the file as published by Hugging Face (LFS oid), when known.
    """

    filename: str
    download_url: str
    size_bytes: int
    shard_index: int
    total_shards: int
    sha256: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        filename = self.filename

        download_url = self.download_url

        size_bytes = self.size_bytes

        shard_index = self.shard_index

        total_shards = self.total_shards

        sha256: None | str | Unset
        if isinstance(self.sha256, Unset):
            sha256 = UNSET
        else:
            sha256 = self.sha256

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "filename": filename,
                "download_url": download_url,
                "size_bytes": size_bytes,
                "shard_index": shard_index,
                "total_shards": total_shards,
            }
        )
        if sha256 is not UNSET:
            field_dict["sha256"] = sha256

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        filename = d.pop("filename")

        download_url = d.pop("download_url")

        size_bytes = d.pop("size_bytes")

        shard_index = d.pop("shard_index")

        total_shards = d.pop("total_shards")

        def _parse_sha256(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        sha256 = _parse_sha256(d.pop("sha256", UNSET))

        hf_shard_info = cls(
            filename=filename,
            download_url=download_url,
            size_bytes=size_bytes,
            shard_index=shard_index,
            total_shards=total_shards,
            sha256=sha256,
        )

        hf_shard_info.additional_properties = d
        return hf_shard_info

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
