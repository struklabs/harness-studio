from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.recommended_backend import RecommendedBackend, check_recommended_backend
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.backend_advice import BackendAdvice
    from ..models.driver_compatibility import DriverCompatibility
    from ..models.gpu_info import GpuInfo


T = TypeVar("T", bound="SystemHardwareInfo")


@_attrs_define
class SystemHardwareInfo:
    """
    Attributes:
        os (str):
        os_version (str):
        arch (str):
        cpu_name (str):
        cpu_cores (int):
        total_ram_bytes (int):
        available_ram_bytes (int):
        gpus (list[GpuInfo]):
        driver_info (DriverCompatibility):
        recommended_backend (RecommendedBackend):
        backend_advice (BackendAdvice): Why a backend is recommended, for the UI (`reason_key` is an i18n key under
            `backend.`).
        primary_gpu_index (int | None | Unset):
        unified_memory_budget_bytes (int | None | Unset): Apple Silicon: memory the GPU may use (Metal
            `recommendedMaxWorkingSetSize`, estimated
            from `hw.memsize` / `iogpu.wired_limit_mb` without Metal; see [`macos_gpu_budget`]).
        intel_mac (bool | Unset): Intel Mac (x86_64 macOS): no Metal build of llama.cpp; CPU only.
    """

    os: str
    os_version: str
    arch: str
    cpu_name: str
    cpu_cores: int
    total_ram_bytes: int
    available_ram_bytes: int
    gpus: list[GpuInfo]
    driver_info: DriverCompatibility
    recommended_backend: RecommendedBackend
    backend_advice: BackendAdvice
    primary_gpu_index: int | None | Unset = UNSET
    unified_memory_budget_bytes: int | None | Unset = UNSET
    intel_mac: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        os = self.os

        os_version = self.os_version

        arch = self.arch

        cpu_name = self.cpu_name

        cpu_cores = self.cpu_cores

        total_ram_bytes = self.total_ram_bytes

        available_ram_bytes = self.available_ram_bytes

        gpus = []
        for gpus_item_data in self.gpus:
            gpus_item = gpus_item_data.to_dict()
            gpus.append(gpus_item)

        driver_info = self.driver_info.to_dict()

        recommended_backend: str = self.recommended_backend

        backend_advice = self.backend_advice.to_dict()

        primary_gpu_index: int | None | Unset
        if isinstance(self.primary_gpu_index, Unset):
            primary_gpu_index = UNSET
        else:
            primary_gpu_index = self.primary_gpu_index

        unified_memory_budget_bytes: int | None | Unset
        if isinstance(self.unified_memory_budget_bytes, Unset):
            unified_memory_budget_bytes = UNSET
        else:
            unified_memory_budget_bytes = self.unified_memory_budget_bytes

        intel_mac = self.intel_mac

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "os": os,
                "os_version": os_version,
                "arch": arch,
                "cpu_name": cpu_name,
                "cpu_cores": cpu_cores,
                "total_ram_bytes": total_ram_bytes,
                "available_ram_bytes": available_ram_bytes,
                "gpus": gpus,
                "driver_info": driver_info,
                "recommended_backend": recommended_backend,
                "backend_advice": backend_advice,
            }
        )
        if primary_gpu_index is not UNSET:
            field_dict["primary_gpu_index"] = primary_gpu_index
        if unified_memory_budget_bytes is not UNSET:
            field_dict["unified_memory_budget_bytes"] = unified_memory_budget_bytes
        if intel_mac is not UNSET:
            field_dict["intel_mac"] = intel_mac

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.backend_advice import BackendAdvice  # noqa: PLC0415
        from ..models.driver_compatibility import DriverCompatibility  # noqa: PLC0415
        from ..models.gpu_info import GpuInfo  # noqa: PLC0415

        d = dict(src_dict)
        os = d.pop("os")

        os_version = d.pop("os_version")

        arch = d.pop("arch")

        cpu_name = d.pop("cpu_name")

        cpu_cores = d.pop("cpu_cores")

        total_ram_bytes = d.pop("total_ram_bytes")

        available_ram_bytes = d.pop("available_ram_bytes")

        gpus = []
        _gpus = d.pop("gpus")
        for gpus_item_data in _gpus:
            gpus_item = GpuInfo.from_dict(gpus_item_data)

            gpus.append(gpus_item)

        driver_info = DriverCompatibility.from_dict(d.pop("driver_info"))

        recommended_backend = check_recommended_backend(d.pop("recommended_backend"))

        backend_advice = BackendAdvice.from_dict(d.pop("backend_advice"))

        def _parse_primary_gpu_index(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        primary_gpu_index = _parse_primary_gpu_index(d.pop("primary_gpu_index", UNSET))

        def _parse_unified_memory_budget_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        unified_memory_budget_bytes = _parse_unified_memory_budget_bytes(
            d.pop("unified_memory_budget_bytes", UNSET)
        )

        intel_mac = d.pop("intel_mac", UNSET)

        system_hardware_info = cls(
            os=os,
            os_version=os_version,
            arch=arch,
            cpu_name=cpu_name,
            cpu_cores=cpu_cores,
            total_ram_bytes=total_ram_bytes,
            available_ram_bytes=available_ram_bytes,
            gpus=gpus,
            driver_info=driver_info,
            recommended_backend=recommended_backend,
            backend_advice=backend_advice,
            primary_gpu_index=primary_gpu_index,
            unified_memory_budget_bytes=unified_memory_budget_bytes,
            intel_mac=intel_mac,
        )

        system_hardware_info.additional_properties = d
        return system_hardware_info

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
