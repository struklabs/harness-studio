from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_11_type import (
    CoreEventType11Type,
    check_core_event_type_11_type,
)

if TYPE_CHECKING:
    from ..models.request_record import RequestRecord


T = TypeVar("T", bound="CoreEventType11")


@_attrs_define
class CoreEventType11:
    """A request passed the gateway (metadata only, never content; analytics stores these).

    Attributes:
        record (RequestRecord): Metadata of one gateway request.
        type_ (CoreEventType11Type):
    """

    record: RequestRecord
    type_: CoreEventType11Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        record = self.record.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "record": record,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.request_record import RequestRecord  # noqa: PLC0415

        d = dict(src_dict)
        record = RequestRecord.from_dict(d.pop("record"))

        type_ = check_core_event_type_11_type(d.pop("type"))

        core_event_type_11 = cls(
            record=record,
            type_=type_,
        )

        core_event_type_11.additional_properties = d
        return core_event_type_11

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
