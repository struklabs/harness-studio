from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="UndoResponse")


@_attrs_define
class UndoResponse:
    """
    Attributes:
        conversation_id (str):
        message_id (str):
        undone (bool):
    """

    conversation_id: str
    message_id: str
    undone: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        conversation_id = self.conversation_id

        message_id = self.message_id

        undone = self.undone

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "conversation_id": conversation_id,
                "message_id": message_id,
                "undone": undone,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        conversation_id = d.pop("conversation_id")

        message_id = d.pop("message_id")

        undone = d.pop("undone")

        undo_response = cls(
            conversation_id=conversation_id,
            message_id=message_id,
            undone=undone,
        )

        undo_response.additional_properties = d
        return undo_response

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
