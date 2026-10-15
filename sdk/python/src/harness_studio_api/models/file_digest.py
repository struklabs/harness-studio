from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="FileDigest")


@_attrs_define
class FileDigest:
    """One hashed file of a runtime.

    Attributes:
        name (str):
        size (int):
        sha256 (str):
        mtime_ms (int | None | Unset):
    """

    name: str
    size: int
    sha256: str
    mtime_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        size = self.size

        sha256 = self.sha256

        mtime_ms: int | None | Unset
        if isinstance(self.mtime_ms, Unset):
            mtime_ms = UNSET
        else:
            mtime_ms = self.mtime_ms

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "size": size,
                "sha256": sha256,
            }
        )
        if mtime_ms is not UNSET:
            field_dict["mtime_ms"] = mtime_ms

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        size = d.pop("size")

        sha256 = d.pop("sha256")

        def _parse_mtime_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        mtime_ms = _parse_mtime_ms(d.pop("mtime_ms", UNSET))

        file_digest = cls(
            name=name,
            size=size,
            sha256=sha256,
            mtime_ms=mtime_ms,
        )

        file_digest.additional_properties = d
        return file_digest

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
