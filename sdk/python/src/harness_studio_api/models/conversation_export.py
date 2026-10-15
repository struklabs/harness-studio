from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.exported_conversation import ExportedConversation
    from ..models.exported_document import ExportedDocument
    from ..models.exported_message import ExportedMessage


T = TypeVar("T", bound="ConversationExport")


@_attrs_define
class ConversationExport:
    """
    Attributes:
        schema (str):
        conversation (ExportedConversation):
        messages (list[ExportedMessage]):
        exported_at (int | Unset):
        documents (list[ExportedDocument] | Unset): Documents attached for local RAG (`ready` ones, with their text and
            chunks). Absent in
            files written before documents were exported.
    """

    schema: str
    conversation: ExportedConversation
    messages: list[ExportedMessage]
    exported_at: int | Unset = UNSET
    documents: list[ExportedDocument] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        schema = self.schema

        conversation = self.conversation.to_dict()

        messages = []
        for messages_item_data in self.messages:
            messages_item = messages_item_data.to_dict()
            messages.append(messages_item)

        exported_at = self.exported_at

        documents: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.documents, Unset):
            documents = []
            for documents_item_data in self.documents:
                documents_item = documents_item_data.to_dict()
                documents.append(documents_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "schema": schema,
                "conversation": conversation,
                "messages": messages,
            }
        )
        if exported_at is not UNSET:
            field_dict["exported_at"] = exported_at
        if documents is not UNSET:
            field_dict["documents"] = documents

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.exported_conversation import ExportedConversation  # noqa: PLC0415
        from ..models.exported_document import ExportedDocument  # noqa: PLC0415
        from ..models.exported_message import ExportedMessage  # noqa: PLC0415

        d = dict(src_dict)
        schema = d.pop("schema")

        conversation = ExportedConversation.from_dict(d.pop("conversation"))

        messages = []
        _messages = d.pop("messages")
        for messages_item_data in _messages:
            messages_item = ExportedMessage.from_dict(messages_item_data)

            messages.append(messages_item)

        exported_at = d.pop("exported_at", UNSET)

        _documents = d.pop("documents", UNSET)
        documents: list[ExportedDocument] | Unset = UNSET
        if _documents is not UNSET:
            documents = []
            for documents_item_data in _documents:
                documents_item = ExportedDocument.from_dict(documents_item_data)

                documents.append(documents_item)

        conversation_export = cls(
            schema=schema,
            conversation=conversation,
            messages=messages,
            exported_at=exported_at,
            documents=documents,
        )

        conversation_export.additional_properties = d
        return conversation_export

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
