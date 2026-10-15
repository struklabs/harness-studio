from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.gpu_summary import GpuSummary


T = TypeVar("T", bound="HardwareSummary")


@_attrs_define
class HardwareSummary:
    """
    Attributes:
        os (str):
        arch (str):
        cpu_name (str):
        cpu_cores (int):
        total_ram_bytes (int):
        gpus (list[GpuSummary]):
        recommended_backend (str):
    """

    os: str
    arch: str
    cpu_name: str
    cpu_cores: int
    total_ram_bytes: int
    gpus: list[GpuSummary]
    recommended_backend: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        os = self.os

        arch = self.arch

        cpu_name = self.cpu_name

        cpu_cores = self.cpu_cores

        total_ram_bytes = self.total_ram_bytes

        gpus = []
        for gpus_item_data in self.gpus:
            gpus_item = gpus_item_data.to_dict()
            gpus.append(gpus_item)

        recommended_backend = self.recommended_backend

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "os": os,
                "arch": arch,
                "cpu_name": cpu_name,
                "cpu_cores": cpu_cores,
                "total_ram_bytes": total_ram_bytes,
                "gpus": gpus,
                "recommended_backend": recommended_backend,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gpu_summary import GpuSummary  # noqa: PLC0415

        d = dict(src_dict)
        os = d.pop("os")

        arch = d.pop("arch")

        cpu_name = d.pop("cpu_name")

        cpu_cores = d.pop("cpu_cores")

        total_ram_bytes = d.pop("total_ram_bytes")

        gpus = []
        _gpus = d.pop("gpus")
        for gpus_item_data in _gpus:
            gpus_item = GpuSummary.from_dict(gpus_item_data)

            gpus.append(gpus_item)

        recommended_backend = d.pop("recommended_backend")

        hardware_summary = cls(
            os=os,
            arch=arch,
            cpu_name=cpu_name,
            cpu_cores=cpu_cores,
            total_ram_bytes=total_ram_bytes,
            gpus=gpus,
            recommended_backend=recommended_backend,
        )

        hardware_summary.additional_properties = d
        return hardware_summary

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
