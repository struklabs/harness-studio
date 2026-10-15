from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="DriverCompatibility")


@_attrs_define
class DriverCompatibility:
    """
    Attributes:
        cuda_supported (bool):
        vulkan_supported (bool):
        metal_supported (bool):
        cuda_driver_version (None | str | Unset):
        rocm_supported (bool | Unset): AMD GPU usable by the official ROCm/HIP build: Windows with an AMD GPU (the
            release
            archive bundles the HIP runtime), Linux with the amdgpu KFD node (`/dev/kfd`).
        wsl (bool | Unset): Running under WSL (CUDA through `/usr/lib/wsl/lib`).
    """

    cuda_supported: bool
    vulkan_supported: bool
    metal_supported: bool
    cuda_driver_version: None | str | Unset = UNSET
    rocm_supported: bool | Unset = UNSET
    wsl: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cuda_supported = self.cuda_supported

        vulkan_supported = self.vulkan_supported

        metal_supported = self.metal_supported

        cuda_driver_version: None | str | Unset
        if isinstance(self.cuda_driver_version, Unset):
            cuda_driver_version = UNSET
        else:
            cuda_driver_version = self.cuda_driver_version

        rocm_supported = self.rocm_supported

        wsl = self.wsl

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cuda_supported": cuda_supported,
                "vulkan_supported": vulkan_supported,
                "metal_supported": metal_supported,
            }
        )
        if cuda_driver_version is not UNSET:
            field_dict["cuda_driver_version"] = cuda_driver_version
        if rocm_supported is not UNSET:
            field_dict["rocm_supported"] = rocm_supported
        if wsl is not UNSET:
            field_dict["wsl"] = wsl

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        cuda_supported = d.pop("cuda_supported")

        vulkan_supported = d.pop("vulkan_supported")

        metal_supported = d.pop("metal_supported")

        def _parse_cuda_driver_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cuda_driver_version = _parse_cuda_driver_version(d.pop("cuda_driver_version", UNSET))

        rocm_supported = d.pop("rocm_supported", UNSET)

        wsl = d.pop("wsl", UNSET)

        driver_compatibility = cls(
            cuda_supported=cuda_supported,
            vulkan_supported=vulkan_supported,
            metal_supported=metal_supported,
            cuda_driver_version=cuda_driver_version,
            rocm_supported=rocm_supported,
            wsl=wsl,
        )

        driver_compatibility.additional_properties = d
        return driver_compatibility

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
