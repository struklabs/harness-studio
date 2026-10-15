from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ExportedConversation")


@_attrs_define
class ExportedConversation:
    """
    Attributes:
        id (str | Unset):  Default: ''.
        title (str | Unset):  Default: ''.
        title_source (str | Unset):  Default: ''.
        pinned (bool | Unset):  Default: False.
        favorite (bool | Unset):  Default: False.
        created_at (int | Unset):  Default: 0.
        updated_at (int | Unset):  Default: 0.
        server_id (None | str | Unset):
        model_alias (None | str | Unset):
        preset_id (None | str | Unset):
        system_prompt (str | Unset):  Default: ''.
        settings (Any | Unset):
        metadata (Any | Unset):
        current_leaf_id (None | str | Unset):
        forked_from_conversation_id (None | str | Unset):
        forked_from_message_id (None | str | Unset):
    """

    id: str | Unset = ""
    title: str | Unset = ""
    title_source: str | Unset = ""
    pinned: bool | Unset = False
    favorite: bool | Unset = False
    created_at: int | Unset = 0
    updated_at: int | Unset = 0
    server_id: None | str | Unset = UNSET
    model_alias: None | str | Unset = UNSET
    preset_id: None | str | Unset = UNSET
    system_prompt: str | Unset = ""
    settings: Any | Unset = UNSET
    metadata: Any | Unset = UNSET
    current_leaf_id: None | str | Unset = UNSET
    forked_from_conversation_id: None | str | Unset = UNSET
    forked_from_message_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        title = self.title

        title_source = self.title_source

        pinned = self.pinned

        favorite = self.favorite

        created_at = self.created_at

        updated_at = self.updated_at

        server_id: None | str | Unset
        if isinstance(self.server_id, Unset):
            server_id = UNSET
        else:
            server_id = self.server_id

        model_alias: None | str | Unset
        if isinstance(self.model_alias, Unset):
            model_alias = UNSET
        else:
            model_alias = self.model_alias

        preset_id: None | str | Unset
        if isinstance(self.preset_id, Unset):
            preset_id = UNSET
        else:
            preset_id = self.preset_id

        system_prompt = self.system_prompt

        settings = self.settings

        metadata = self.metadata

        current_leaf_id: None | str | Unset
        if isinstance(self.current_leaf_id, Unset):
            current_leaf_id = UNSET
        else:
            current_leaf_id = self.current_leaf_id

        forked_from_conversation_id: None | str | Unset
        if isinstance(self.forked_from_conversation_id, Unset):
            forked_from_conversation_id = UNSET
        else:
            forked_from_conversation_id = self.forked_from_conversation_id

        forked_from_message_id: None | str | Unset
        if isinstance(self.forked_from_message_id, Unset):
            forked_from_message_id = UNSET
        else:
            forked_from_message_id = self.forked_from_message_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if id is not UNSET:
            field_dict["id"] = id
        if title is not UNSET:
            field_dict["title"] = title
        if title_source is not UNSET:
            field_dict["title_source"] = title_source
        if pinned is not UNSET:
            field_dict["pinned"] = pinned
        if favorite is not UNSET:
            field_dict["favorite"] = favorite
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at
        if server_id is not UNSET:
            field_dict["server_id"] = server_id
        if model_alias is not UNSET:
            field_dict["model_alias"] = model_alias
        if preset_id is not UNSET:
            field_dict["preset_id"] = preset_id
        if system_prompt is not UNSET:
            field_dict["system_prompt"] = system_prompt
        if settings is not UNSET:
            field_dict["settings"] = settings
        if metadata is not UNSET:
            field_dict["metadata"] = metadata
        if current_leaf_id is not UNSET:
            field_dict["current_leaf_id"] = current_leaf_id
        if forked_from_conversation_id is not UNSET:
            field_dict["forked_from_conversation_id"] = forked_from_conversation_id
        if forked_from_message_id is not UNSET:
            field_dict["forked_from_message_id"] = forked_from_message_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id", UNSET)

        title = d.pop("title", UNSET)

        title_source = d.pop("title_source", UNSET)

        pinned = d.pop("pinned", UNSET)

        favorite = d.pop("favorite", UNSET)

        created_at = d.pop("created_at", UNSET)

        updated_at = d.pop("updated_at", UNSET)

        def _parse_server_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        server_id = _parse_server_id(d.pop("server_id", UNSET))

        def _parse_model_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model_alias = _parse_model_alias(d.pop("model_alias", UNSET))

        def _parse_preset_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        preset_id = _parse_preset_id(d.pop("preset_id", UNSET))

        system_prompt = d.pop("system_prompt", UNSET)

        settings = d.pop("settings", UNSET)

        metadata = d.pop("metadata", UNSET)

        def _parse_current_leaf_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        current_leaf_id = _parse_current_leaf_id(d.pop("current_leaf_id", UNSET))

        def _parse_forked_from_conversation_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        forked_from_conversation_id = _parse_forked_from_conversation_id(
            d.pop("forked_from_conversation_id", UNSET)
        )

        def _parse_forked_from_message_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        forked_from_message_id = _parse_forked_from_message_id(
            d.pop("forked_from_message_id", UNSET)
        )

        exported_conversation = cls(
            id=id,
            title=title,
            title_source=title_source,
            pinned=pinned,
            favorite=favorite,
            created_at=created_at,
            updated_at=updated_at,
            server_id=server_id,
            model_alias=model_alias,
            preset_id=preset_id,
            system_prompt=system_prompt,
            settings=settings,
            metadata=metadata,
            current_leaf_id=current_leaf_id,
            forked_from_conversation_id=forked_from_conversation_id,
            forked_from_message_id=forked_from_message_id,
        )

        exported_conversation.additional_properties = d
        return exported_conversation

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
