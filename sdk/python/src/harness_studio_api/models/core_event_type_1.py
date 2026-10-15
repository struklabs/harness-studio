from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_1_type import (
    CoreEventType1Type,
    check_core_event_type_1_type,
)

T = TypeVar("T", bound="CoreEventType1")


@_attrs_define
class CoreEventType1:
    """The tool registry changed (generated, installed or MCP tools).

    Attributes:
        type_ (CoreEventType1Type):
    """

    type_: CoreEventType1Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        type_ = check_core_event_type_1_type(d.pop("type"))

        core_event_type_1 = cls(
            type_=type_,
        )

        core_event_type_1.additional_properties = d
        return core_event_type_1

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
