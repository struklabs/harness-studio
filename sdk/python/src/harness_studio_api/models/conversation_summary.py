from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ConversationSummary")


@_attrs_define
class ConversationSummary:
    """One row of the conversation list.

    Attributes:
        id (str):
        title (str):
        title_source (str):
        pinned (bool):
        favorite (bool):
        created_at (int):
        updated_at (int):
        folder_id (None | str | Unset):
        last_opened_at (int | None | Unset):
        model_alias (None | str | Unset):
        snippet (None | str | Unset): Matching text fragment when the list comes from a search.
    """

    id: str
    title: str
    title_source: str
    pinned: bool
    favorite: bool
    created_at: int
    updated_at: int
    folder_id: None | str | Unset = UNSET
    last_opened_at: int | None | Unset = UNSET
    model_alias: None | str | Unset = UNSET
    snippet: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        title = self.title

        title_source = self.title_source

        pinned = self.pinned

        favorite = self.favorite

        created_at = self.created_at

        updated_at = self.updated_at

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

        model_alias: None | str | Unset
        if isinstance(self.model_alias, Unset):
            model_alias = UNSET
        else:
            model_alias = self.model_alias

        snippet: None | str | Unset
        if isinstance(self.snippet, Unset):
            snippet = UNSET
        else:
            snippet = self.snippet

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
            }
        )
        if folder_id is not UNSET:
            field_dict["folder_id"] = folder_id
        if last_opened_at is not UNSET:
            field_dict["last_opened_at"] = last_opened_at
        if model_alias is not UNSET:
            field_dict["model_alias"] = model_alias
        if snippet is not UNSET:
            field_dict["snippet"] = snippet

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

        def _parse_model_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model_alias = _parse_model_alias(d.pop("model_alias", UNSET))

        def _parse_snippet(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        snippet = _parse_snippet(d.pop("snippet", UNSET))

        conversation_summary = cls(
            id=id,
            title=title,
            title_source=title_source,
            pinned=pinned,
            favorite=favorite,
            created_at=created_at,
            updated_at=updated_at,
            folder_id=folder_id,
            last_opened_at=last_opened_at,
            model_alias=model_alias,
            snippet=snippet,
        )

        conversation_summary.additional_properties = d
        return conversation_summary

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
