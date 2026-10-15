from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_12_type import (
    CoreEventType12Type,
    check_core_event_type_12_type,
)
from ..types import UNSET, Unset

T = TypeVar("T", bound="CoreEventType12")


@_attrs_define
class CoreEventType12:
    """The gateway scheduler loaded, unloaded or evicted a model.

    Attributes:
        definition_id (str):
        alias (str):
        server_id (str):
        action (str): `loaded`, `unloaded_ttl`, `evicted`, `unloaded`, `load_failed`.
        type_ (CoreEventType12Type):
        reason (None | str | Unset): Why (the alias that needed the room, the failure code...).
    """

    definition_id: str
    alias: str
    server_id: str
    action: str
    type_: CoreEventType12Type
    reason: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        definition_id = self.definition_id

        alias = self.alias

        server_id = self.server_id

        action = self.action

        type_: str = self.type_

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "definition_id": definition_id,
                "alias": alias,
                "server_id": server_id,
                "action": action,
                "type": type_,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        definition_id = d.pop("definition_id")

        alias = d.pop("alias")

        server_id = d.pop("server_id")

        action = d.pop("action")

        type_ = check_core_event_type_12_type(d.pop("type"))

        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))

        core_event_type_12 = cls(
            definition_id=definition_id,
            alias=alias,
            server_id=server_id,
            action=action,
            type_=type_,
            reason=reason,
        )

        core_event_type_12.additional_properties = d
        return core_event_type_12

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
