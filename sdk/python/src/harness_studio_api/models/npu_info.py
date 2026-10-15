from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="NpuInfo")


@_attrs_define
class NpuInfo:
    """What the NPU detection found.

    Attributes:
        detected (bool): An AMD XDNA NPU is present.
        driver_installed (bool): The driver is installed (Windows: the device is named "NPU Compute Accelerator Device"
            / class `ComputeAccelerator`; Linux: the `amdxdna` module is bound). Without it the
            NPU exists but no backend can use it.
        generation (None | str | Unset): `xdna` (Phoenix, Hawk Point) or `xdna2` (Strix, Krackan).
        name (None | str | Unset): Device name as the OS reports it.
        hardware_id (None | str | Unset): The hardware id that matched (`PCI\\VEN_1022&DEV_17F0`).
    """

    detected: bool
    driver_installed: bool
    generation: None | str | Unset = UNSET
    name: None | str | Unset = UNSET
    hardware_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        detected = self.detected

        driver_installed = self.driver_installed

        generation: None | str | Unset
        if isinstance(self.generation, Unset):
            generation = UNSET
        else:
            generation = self.generation

        name: None | str | Unset
        if isinstance(self.name, Unset):
            name = UNSET
        else:
            name = self.name

        hardware_id: None | str | Unset
        if isinstance(self.hardware_id, Unset):
            hardware_id = UNSET
        else:
            hardware_id = self.hardware_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "detected": detected,
                "driver_installed": driver_installed,
            }
        )
        if generation is not UNSET:
            field_dict["generation"] = generation
        if name is not UNSET:
            field_dict["name"] = name
        if hardware_id is not UNSET:
            field_dict["hardware_id"] = hardware_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        detected = d.pop("detected")

        driver_installed = d.pop("driver_installed")

        def _parse_generation(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        generation = _parse_generation(d.pop("generation", UNSET))

        def _parse_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        name = _parse_name(d.pop("name", UNSET))

        def _parse_hardware_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        hardware_id = _parse_hardware_id(d.pop("hardware_id", UNSET))

        npu_info = cls(
            detected=detected,
            driver_installed=driver_installed,
            generation=generation,
            name=name,
            hardware_id=hardware_id,
        )

        npu_info.additional_properties = d
        return npu_info

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
