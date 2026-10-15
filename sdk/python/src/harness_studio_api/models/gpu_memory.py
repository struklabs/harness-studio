from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.memory_breakdown import MemoryBreakdown


T = TypeVar("T", bound="GpuMemory")


@_attrs_define
class GpuMemory:
    """VRAM of one graphics adapter (a machine with several: discrete card plus integrated GPU).

    Attributes:
        index (int):
        name (str):
        used_bytes (int):
        total_bytes (int):
        breakdown (MemoryBreakdown | None | Unset):
    """

    index: int
    name: str
    used_bytes: int
    total_bytes: int
    breakdown: MemoryBreakdown | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.memory_breakdown import MemoryBreakdown  # noqa: PLC0415

        index = self.index

        name = self.name

        used_bytes = self.used_bytes

        total_bytes = self.total_bytes

        breakdown: dict[str, Any] | None | Unset
        if isinstance(self.breakdown, Unset):
            breakdown = UNSET
        elif isinstance(self.breakdown, MemoryBreakdown):
            breakdown = self.breakdown.to_dict()
        else:
            breakdown = self.breakdown

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "index": index,
                "name": name,
                "used_bytes": used_bytes,
                "total_bytes": total_bytes,
            }
        )
        if breakdown is not UNSET:
            field_dict["breakdown"] = breakdown

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.memory_breakdown import MemoryBreakdown  # noqa: PLC0415

        d = dict(src_dict)
        index = d.pop("index")

        name = d.pop("name")

        used_bytes = d.pop("used_bytes")

        total_bytes = d.pop("total_bytes")

        def _parse_breakdown(data: object) -> MemoryBreakdown | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                breakdown_type_1 = MemoryBreakdown.from_dict(data)

                return breakdown_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MemoryBreakdown | None | Unset, data)

        breakdown = _parse_breakdown(d.pop("breakdown", UNSET))

        gpu_memory = cls(
            index=index,
            name=name,
            used_bytes=used_bytes,
            total_bytes=total_bytes,
            breakdown=breakdown,
        )

        gpu_memory.additional_properties = d
        return gpu_memory

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
