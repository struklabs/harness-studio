from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="QuantEstimate")


@_attrs_define
class QuantEstimate:
    """The result size and disk need of one target type.

    Attributes:
        quant (str):
        size_bytes (int): Size of the finished GGUF.
        disk_bytes (int): Disk the whole run needs at its peak (download + 16-bit GGUF + result).
    """

    quant: str
    size_bytes: int
    disk_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        quant = self.quant

        size_bytes = self.size_bytes

        disk_bytes = self.disk_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "quant": quant,
                "size_bytes": size_bytes,
                "disk_bytes": disk_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        quant = d.pop("quant")

        size_bytes = d.pop("size_bytes")

        disk_bytes = d.pop("disk_bytes")

        quant_estimate = cls(
            quant=quant,
            size_bytes=size_bytes,
            disk_bytes=disk_bytes,
        )

        quant_estimate.additional_properties = d
        return quant_estimate

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
