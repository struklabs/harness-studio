from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RuntimeDevice")


@_attrs_define
class RuntimeDevice:
    """A GPU device of the runtime.

    Attributes:
        id (str): `Vulkan0`, `CUDA1`, `ROCm0`, `Metal`, `SYCL0`...
        index (int): Position in the runtime's list (the value `--main-gpu` takes; `--tensor-split` order).
        backend (str): `Vulkan`, `CUDA`, `ROCm`, `Metal`, `SYCL`...
        name (str):
        integrated (bool): Integrated GPU (shares system memory); never part of a split suggestion.
        total_bytes (int | None | Unset):
        free_bytes (int | None | Unset): Free memory as the runtime reports it.
        os_used_bytes (int | None | Unset): Current usage of the matching OS adapter (all processes), when known.
        available_bytes (int | None | Unset): What a new model can use: the smaller of the runtime's free value and
            `total - OS usage` (CUDA on Windows: `total - OS usage` alone, see `available_from`).
    """

    id: str
    index: int
    backend: str
    name: str
    integrated: bool
    total_bytes: int | None | Unset = UNSET
    free_bytes: int | None | Unset = UNSET
    os_used_bytes: int | None | Unset = UNSET
    available_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        index = self.index

        backend = self.backend

        name = self.name

        integrated = self.integrated

        total_bytes: int | None | Unset
        if isinstance(self.total_bytes, Unset):
            total_bytes = UNSET
        else:
            total_bytes = self.total_bytes

        free_bytes: int | None | Unset
        if isinstance(self.free_bytes, Unset):
            free_bytes = UNSET
        else:
            free_bytes = self.free_bytes

        os_used_bytes: int | None | Unset
        if isinstance(self.os_used_bytes, Unset):
            os_used_bytes = UNSET
        else:
            os_used_bytes = self.os_used_bytes

        available_bytes: int | None | Unset
        if isinstance(self.available_bytes, Unset):
            available_bytes = UNSET
        else:
            available_bytes = self.available_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "index": index,
                "backend": backend,
                "name": name,
                "integrated": integrated,
            }
        )
        if total_bytes is not UNSET:
            field_dict["total_bytes"] = total_bytes
        if free_bytes is not UNSET:
            field_dict["free_bytes"] = free_bytes
        if os_used_bytes is not UNSET:
            field_dict["os_used_bytes"] = os_used_bytes
        if available_bytes is not UNSET:
            field_dict["available_bytes"] = available_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        index = d.pop("index")

        backend = d.pop("backend")

        name = d.pop("name")

        integrated = d.pop("integrated")

        def _parse_total_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_bytes = _parse_total_bytes(d.pop("total_bytes", UNSET))

        def _parse_free_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        free_bytes = _parse_free_bytes(d.pop("free_bytes", UNSET))

        def _parse_os_used_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        os_used_bytes = _parse_os_used_bytes(d.pop("os_used_bytes", UNSET))

        def _parse_available_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        available_bytes = _parse_available_bytes(d.pop("available_bytes", UNSET))

        runtime_device = cls(
            id=id,
            index=index,
            backend=backend,
            name=name,
            integrated=integrated,
            total_bytes=total_bytes,
            free_bytes=free_bytes,
            os_used_bytes=os_used_bytes,
            available_bytes=available_bytes,
        )

        runtime_device.additional_properties = d
        return runtime_device

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
