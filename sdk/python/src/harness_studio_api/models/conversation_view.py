from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.conversation import Conversation
    from ..models.path_message import PathMessage


T = TypeVar("T", bound="ConversationView")


@_attrs_define
class ConversationView:
    """
    Attributes:
        conversation (Conversation):
        messages (list[PathMessage]): Root → current leaf.
    """

    conversation: Conversation
    messages: list[PathMessage]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        conversation = self.conversation.to_dict()

        messages = []
        for messages_item_data in self.messages:
            messages_item = messages_item_data.to_dict()
            messages.append(messages_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "conversation": conversation,
                "messages": messages,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.conversation import Conversation  # noqa: PLC0415
        from ..models.path_message import PathMessage  # noqa: PLC0415

        d = dict(src_dict)
        conversation = Conversation.from_dict(d.pop("conversation"))

        messages = []
        _messages = d.pop("messages")
        for messages_item_data in _messages:
            messages_item = PathMessage.from_dict(messages_item_data)

            messages.append(messages_item)

        conversation_view = cls(
            conversation=conversation,
            messages=messages,
        )

        conversation_view.additional_properties = d
        return conversation_view

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
