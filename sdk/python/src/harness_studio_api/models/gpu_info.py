from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.gpu_vendor_type_0 import GpuVendorType0, check_gpu_vendor_type_0
from ..models.gpu_vendor_type_1 import GpuVendorType1, check_gpu_vendor_type_1
from ..models.gpu_vendor_type_2 import GpuVendorType2, check_gpu_vendor_type_2
from ..models.gpu_vendor_type_3 import GpuVendorType3, check_gpu_vendor_type_3
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.gpu_vendor_type_4 import GpuVendorType4


T = TypeVar("T", bound="GpuInfo")


@_attrs_define
class GpuInfo:
    """
    Attributes:
        index (int):
        name (str):
        vendor (GpuVendorType0 | GpuVendorType1 | GpuVendorType2 | GpuVendorType3 | GpuVendorType4):
        is_discrete (bool):
        is_default (bool):
        vram_bytes (int | None | Unset):
        driver_version (None | str | Unset):
    """

    index: int
    name: str
    vendor: GpuVendorType0 | GpuVendorType1 | GpuVendorType2 | GpuVendorType3 | GpuVendorType4
    is_discrete: bool
    is_default: bool
    vram_bytes: int | None | Unset = UNSET
    driver_version: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        index = self.index

        name = self.name

        vendor: dict[str, Any] | str
        if isinstance(self.vendor, str):
            vendor = self.vendor
        elif isinstance(self.vendor, str):
            vendor = self.vendor
        elif isinstance(self.vendor, str):
            vendor = self.vendor
        elif isinstance(self.vendor, str):
            vendor = self.vendor
        else:
            vendor = self.vendor.to_dict()

        is_discrete = self.is_discrete

        is_default = self.is_default

        vram_bytes: int | None | Unset
        if isinstance(self.vram_bytes, Unset):
            vram_bytes = UNSET
        else:
            vram_bytes = self.vram_bytes

        driver_version: None | str | Unset
        if isinstance(self.driver_version, Unset):
            driver_version = UNSET
        else:
            driver_version = self.driver_version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "index": index,
                "name": name,
                "vendor": vendor,
                "is_discrete": is_discrete,
                "is_default": is_default,
            }
        )
        if vram_bytes is not UNSET:
            field_dict["vram_bytes"] = vram_bytes
        if driver_version is not UNSET:
            field_dict["driver_version"] = driver_version

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gpu_vendor_type_4 import GpuVendorType4  # noqa: PLC0415

        d = dict(src_dict)
        index = d.pop("index")

        name = d.pop("name")

        def _parse_vendor(
            data: object,
        ) -> GpuVendorType0 | GpuVendorType1 | GpuVendorType2 | GpuVendorType3 | GpuVendorType4:
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_gpu_vendor_type_0 = check_gpu_vendor_type_0(data)

                return componentsschemas_gpu_vendor_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_gpu_vendor_type_1 = check_gpu_vendor_type_1(data)

                return componentsschemas_gpu_vendor_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_gpu_vendor_type_2 = check_gpu_vendor_type_2(data)

                return componentsschemas_gpu_vendor_type_2
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_gpu_vendor_type_3 = check_gpu_vendor_type_3(data)

                return componentsschemas_gpu_vendor_type_3
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_gpu_vendor_type_4 = GpuVendorType4.from_dict(data)

            return componentsschemas_gpu_vendor_type_4

        vendor = _parse_vendor(d.pop("vendor"))

        is_discrete = d.pop("is_discrete")

        is_default = d.pop("is_default")

        def _parse_vram_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vram_bytes = _parse_vram_bytes(d.pop("vram_bytes", UNSET))

        def _parse_driver_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        driver_version = _parse_driver_version(d.pop("driver_version", UNSET))

        gpu_info = cls(
            index=index,
            name=name,
            vendor=vendor,
            is_discrete=is_discrete,
            is_default=is_default,
            vram_bytes=vram_bytes,
            driver_version=driver_version,
        )

        gpu_info.additional_properties = d
        return gpu_info

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
