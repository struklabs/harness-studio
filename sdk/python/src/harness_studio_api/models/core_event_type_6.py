from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_6_type import (
    CoreEventType6Type,
    check_core_event_type_6_type,
)

T = TypeVar("T", bound="CoreEventType6")


@_attrs_define
class CoreEventType6:
    """A conversion job was removed from the list.

    Attributes:
        id (int):
        type_ (CoreEventType6Type):
    """

    id: int
    type_: CoreEventType6Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        type_ = check_core_event_type_6_type(d.pop("type"))

        core_event_type_6 = cls(
            id=id,
            type_=type_,
        )

        core_event_type_6.additional_properties = d
        return core_event_type_6

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
