from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.compatibility_type_3_status import (
    CompatibilityType3Status,
    check_compatibility_type_3_status,
)
from ..models.model_format import ModelFormat, check_model_format

T = TypeVar("T", bound="CompatibilityType3")


@_attrs_define
class CompatibilityType3:
    """No Harness runtime runs this format.

    Attributes:
        format_ (ModelFormat): The format a repo's model files are in.
        status (CompatibilityType3Status):
    """

    format_: ModelFormat
    status: CompatibilityType3Status
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        format_: str = self.format_

        status: str = self.status

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "format": format_,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        format_ = check_model_format(d.pop("format"))

        status = check_compatibility_type_3_status(d.pop("status"))

        compatibility_type_3 = cls(
            format_=format_,
            status=status,
        )

        compatibility_type_3.additional_properties = d
        return compatibility_type_3

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
