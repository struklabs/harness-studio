from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.compatibility_type_2_status import (
    CompatibilityType2Status,
    check_compatibility_type_2_status,
)

T = TypeVar("T", bound="CompatibilityType2")


@_attrs_define
class CompatibilityType2:
    """The family that runs it has no build for this OS / architecture (ONNX Runtime on an
    Intel Mac): nothing to install, the model cannot run here.

        Attributes:
            runtime (str):
            os (str):
            arch (str):
            status (CompatibilityType2Status):
    """

    runtime: str
    os: str
    arch: str
    status: CompatibilityType2Status
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        runtime = self.runtime

        os = self.os

        arch = self.arch

        status: str = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "runtime": runtime,
                "os": os,
                "arch": arch,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        runtime = d.pop("runtime")

        os = d.pop("os")

        arch = d.pop("arch")

        status = check_compatibility_type_2_status(d.pop("status"))

        compatibility_type_2 = cls(
            runtime=runtime,
            os=os,
            arch=arch,
            status=status,
        )

        compatibility_type_2.additional_properties = d
        return compatibility_type_2

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
