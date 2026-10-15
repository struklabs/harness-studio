from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="Device")


@_attrs_define
class Device:
    """A backend device from `--list-devices`.

    Attributes:
        id (str): `Vulkan0`, `CUDA1`, ...
        backend (str): `Vulkan`, `CUDA`, `ROCm`, `Metal`, `SYCL`, ...
        name (str):
        total_mib (int | None | Unset):
        free_mib (int | None | Unset):
    """

    id: str
    backend: str
    name: str
    total_mib: int | None | Unset = UNSET
    free_mib: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        backend = self.backend

        name = self.name

        total_mib: int | None | Unset
        if isinstance(self.total_mib, Unset):
            total_mib = UNSET
        else:
            total_mib = self.total_mib

        free_mib: int | None | Unset
        if isinstance(self.free_mib, Unset):
            free_mib = UNSET
        else:
            free_mib = self.free_mib

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "backend": backend,
                "name": name,
            }
        )
        if total_mib is not UNSET:
            field_dict["total_mib"] = total_mib
        if free_mib is not UNSET:
            field_dict["free_mib"] = free_mib

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        backend = d.pop("backend")

        name = d.pop("name")

        def _parse_total_mib(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_mib = _parse_total_mib(d.pop("total_mib", UNSET))

        def _parse_free_mib(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        free_mib = _parse_free_mib(d.pop("free_mib", UNSET))

        device = cls(
            id=id,
            backend=backend,
            name=name,
            total_mib=total_mib,
            free_mib=free_mib,
        )

        device.additional_properties = d
        return device

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
