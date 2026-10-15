from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="Conversation")


@_attrs_define
class Conversation:
    """
    Attributes:
        id (str):
        title (str):
        title_source (str):
        pinned (bool):
        favorite (bool):
        created_at (int):
        updated_at (int):
        system_prompt (str):
        settings (Any): Sampling snapshot (opaque JSON object owned by the client).
        metadata (Any):
        folder_id (None | str | Unset):
        last_opened_at (int | None | Unset):
        server_id (None | str | Unset):
        model_alias (None | str | Unset):
        preset_id (None | str | Unset):
        current_leaf_id (None | str | Unset):
        forked_from_conversation_id (None | str | Unset):
        forked_from_message_id (None | str | Unset):
    """

    id: str
    title: str
    title_source: str
    pinned: bool
    favorite: bool
    created_at: int
    updated_at: int
    system_prompt: str
    settings: Any
    metadata: Any
    folder_id: None | str | Unset = UNSET
    last_opened_at: int | None | Unset = UNSET
    server_id: None | str | Unset = UNSET
    model_alias: None | str | Unset = UNSET
    preset_id: None | str | Unset = UNSET
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

        system_prompt = self.system_prompt

        settings = self.settings

        metadata = self.metadata

        folder_id: None | str | Unset
        if isinstance(self.folder_id, Unset):
            folder_id = UNSET
        else:
            folder_id = self.folder_id

        last_opened_at: int | None | Unset
        if isinstance(self.last_opened_at, Unset):
            last_opened_at = UNSET
        else:
            last_opened_at = self.last_opened_at

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
        field_dict.update(
            {
                "id": id,
                "title": title,
                "title_source": title_source,
                "pinned": pinned,
                "favorite": favorite,
                "created_at": created_at,
                "updated_at": updated_at,
                "system_prompt": system_prompt,
                "settings": settings,
                "metadata": metadata,
            }
        )
        if folder_id is not UNSET:
            field_dict["folder_id"] = folder_id
        if last_opened_at is not UNSET:
            field_dict["last_opened_at"] = last_opened_at
        if server_id is not UNSET:
            field_dict["server_id"] = server_id
        if model_alias is not UNSET:
            field_dict["model_alias"] = model_alias
        if preset_id is not UNSET:
            field_dict["preset_id"] = preset_id
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
        id = d.pop("id")

        title = d.pop("title")

        title_source = d.pop("title_source")

        pinned = d.pop("pinned")

        favorite = d.pop("favorite")

        created_at = d.pop("created_at")

        updated_at = d.pop("updated_at")

        system_prompt = d.pop("system_prompt")

        settings = d.pop("settings")

        metadata = d.pop("metadata")

        def _parse_folder_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        folder_id = _parse_folder_id(d.pop("folder_id", UNSET))

        def _parse_last_opened_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        last_opened_at = _parse_last_opened_at(d.pop("last_opened_at", UNSET))

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

        conversation = cls(
            id=id,
            title=title,
            title_source=title_source,
            pinned=pinned,
            favorite=favorite,
            created_at=created_at,
            updated_at=updated_at,
            system_prompt=system_prompt,
            settings=settings,
            metadata=metadata,
            folder_id=folder_id,
            last_opened_at=last_opened_at,
            server_id=server_id,
            model_alias=model_alias,
            preset_id=preset_id,
            current_leaf_id=current_leaf_id,
            forked_from_conversation_id=forked_from_conversation_id,
            forked_from_message_id=forked_from_message_id,
        )

        conversation.additional_properties = d
        return conversation

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
