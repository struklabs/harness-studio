from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_14_type import (
    CoreEventType14Type,
    check_core_event_type_14_type,
)

T = TypeVar("T", bound="CoreEventType14")


@_attrs_define
class CoreEventType14:
    """Gateway request records were written to `analytics.db` (they wait
    [`crate::analytics::ENRICH_DELAY`] after `gateway_request`); views reading the request
    log reload on this, not on `gateway_request`.

        Attributes:
            count (int): Records in the batch.
            type_ (CoreEventType14Type):
    """

    count: int
    type_: CoreEventType14Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        count = self.count

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "count": count,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        count = d.pop("count")

        type_ = check_core_event_type_14_type(d.pop("type"))

        core_event_type_14 = cls(
            count=count,
            type_=type_,
        )

        core_event_type_14.additional_properties = d
        return core_event_type_14

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
