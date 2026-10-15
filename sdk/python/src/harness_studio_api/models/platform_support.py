from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="PlatformSupport")


@_attrs_define
class PlatformSupport:
    """[`supported`] with the OS and architecture it was decided for (the UI names them).

    Attributes:
        supported (bool):
        os (str): `std::env::consts::OS` (`macos`, `windows`, `linux`).
        arch (str): `std::env::consts::ARCH` (`x86_64`, `aarch64`).
    """

    supported: bool
    os: str
    arch: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        supported = self.supported

        os = self.os

        arch = self.arch

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "supported": supported,
                "os": os,
                "arch": arch,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        supported = d.pop("supported")

        os = d.pop("os")

        arch = d.pop("arch")

        platform_support = cls(
            supported=supported,
            os=os,
            arch=arch,
        )

        platform_support.additional_properties = d
        return platform_support

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
