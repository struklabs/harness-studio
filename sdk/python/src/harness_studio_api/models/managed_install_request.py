from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ManagedInstallRequest")


@_attrs_define
class ManagedInstallRequest:
    """
    Attributes:
        backend (None | str | Unset): `vulkan`, `rocm`, `cuda`, `sycl`, `cpu`, `metal`: exactly this backend (a switch).
            Omitted: the installed build's backend (an update), else the hardware recommendation.
        allow_unverified (bool | Unset): Install archives that have no SHA-256 digest on GitHub.
    """

    backend: None | str | Unset = UNSET
    allow_unverified: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        allow_unverified = self.allow_unverified

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if backend is not UNSET:
            field_dict["backend"] = backend
        if allow_unverified is not UNSET:
            field_dict["allow_unverified"] = allow_unverified

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        allow_unverified = d.pop("allow_unverified", UNSET)

        managed_install_request = cls(
            backend=backend,
            allow_unverified=allow_unverified,
        )

        managed_install_request.additional_properties = d
        return managed_install_request

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
