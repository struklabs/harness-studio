from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RemoteResource")


@_attrs_define
class RemoteResource:
    """A resource the server offers (`resources/list`).

    Attributes:
        uri (str):
        name (str):
        title (None | str | Unset):
        description (None | str | Unset):
        mime_type (None | str | Unset):
        size (int | None | Unset):
    """

    uri: str
    name: str
    title: None | str | Unset = UNSET
    description: None | str | Unset = UNSET
    mime_type: None | str | Unset = UNSET
    size: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        uri = self.uri

        name = self.name

        title: None | str | Unset
        if isinstance(self.title, Unset):
            title = UNSET
        else:
            title = self.title

        description: None | str | Unset
        if isinstance(self.description, Unset):
            description = UNSET
        else:
            description = self.description

        mime_type: None | str | Unset
        if isinstance(self.mime_type, Unset):
            mime_type = UNSET
        else:
            mime_type = self.mime_type

        size: int | None | Unset
        if isinstance(self.size, Unset):
            size = UNSET
        else:
            size = self.size

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "uri": uri,
                "name": name,
            }
        )
        if title is not UNSET:
            field_dict["title"] = title
        if description is not UNSET:
            field_dict["description"] = description
        if mime_type is not UNSET:
            field_dict["mime_type"] = mime_type
        if size is not UNSET:
            field_dict["size"] = size

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        uri = d.pop("uri")

        name = d.pop("name")

        def _parse_title(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        title = _parse_title(d.pop("title", UNSET))

        def _parse_description(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        description = _parse_description(d.pop("description", UNSET))

        def _parse_mime_type(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mime_type = _parse_mime_type(d.pop("mime_type", UNSET))

        def _parse_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        size = _parse_size(d.pop("size", UNSET))

        remote_resource = cls(
            uri=uri,
            name=name,
            title=title,
            description=description,
            mime_type=mime_type,
            size=size,
        )

        remote_resource.additional_properties = d
        return remote_resource

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
