from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_13_type import (
    CoreEventType13Type,
    check_core_event_type_13_type,
)

T = TypeVar("T", bound="CoreEventType13")


@_attrs_define
class CoreEventType13:
    """The Dashboard server's launch was saved as the last session (after it reached
    Running); views showing the saved launch reload on this.

        Attributes:
            server_id (str):
            type_ (CoreEventType13Type):
    """

    server_id: str
    type_: CoreEventType13Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_id": server_id,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        server_id = d.pop("server_id")

        type_ = check_core_event_type_13_type(d.pop("type"))

        core_event_type_13 = cls(
            server_id=server_id,
            type_=type_,
        )

        core_event_type_13.additional_properties = d
        return core_event_type_13

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
