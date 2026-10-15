from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="GpuSummary")


@_attrs_define
class GpuSummary:
    """
    Attributes:
        name (str):
        vram_bytes (int | None | Unset):
    """

    name: str
    vram_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        vram_bytes: int | None | Unset
        if isinstance(self.vram_bytes, Unset):
            vram_bytes = UNSET
        else:
            vram_bytes = self.vram_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
            }
        )
        if vram_bytes is not UNSET:
            field_dict["vram_bytes"] = vram_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        def _parse_vram_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vram_bytes = _parse_vram_bytes(d.pop("vram_bytes", UNSET))

        gpu_summary = cls(
            name=name,
            vram_bytes=vram_bytes,
        )

        gpu_summary.additional_properties = d
        return gpu_summary

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
