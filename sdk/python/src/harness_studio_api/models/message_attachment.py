from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="MessageAttachment")


@_attrs_define
class MessageAttachment:
    """Attachment metadata (the file itself is not part of the database or the export).

    Attributes:
        id (str | Unset):  Default: ''.
        kind (str | Unset):  Default: ''.
        name (str | Unset):  Default: ''.
        mime (None | str | Unset):
        size (int | None | Unset):
        sha256 (None | str | Unset):
        path (None | str | Unset):
        created_at (int | Unset):  Default: 0.
    """

    id: str | Unset = ""
    kind: str | Unset = ""
    name: str | Unset = ""
    mime: None | str | Unset = UNSET
    size: int | None | Unset = UNSET
    sha256: None | str | Unset = UNSET
    path: None | str | Unset = UNSET
    created_at: int | Unset = 0
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        kind = self.kind

        name = self.name

        mime: None | str | Unset
        if isinstance(self.mime, Unset):
            mime = UNSET
        else:
            mime = self.mime

        size: int | None | Unset
        if isinstance(self.size, Unset):
            size = UNSET
        else:
            size = self.size

        sha256: None | str | Unset
        if isinstance(self.sha256, Unset):
            sha256 = UNSET
        else:
            sha256 = self.sha256

        path: None | str | Unset
        if isinstance(self.path, Unset):
            path = UNSET
        else:
            path = self.path

        created_at = self.created_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if id is not UNSET:
            field_dict["id"] = id
        if kind is not UNSET:
            field_dict["kind"] = kind
        if name is not UNSET:
            field_dict["name"] = name
        if mime is not UNSET:
            field_dict["mime"] = mime
        if size is not UNSET:
            field_dict["size"] = size
        if sha256 is not UNSET:
            field_dict["sha256"] = sha256
        if path is not UNSET:
            field_dict["path"] = path
        if created_at is not UNSET:
            field_dict["created_at"] = created_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id", UNSET)

        kind = d.pop("kind", UNSET)

        name = d.pop("name", UNSET)

        def _parse_mime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mime = _parse_mime(d.pop("mime", UNSET))

        def _parse_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        size = _parse_size(d.pop("size", UNSET))

        def _parse_sha256(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        sha256 = _parse_sha256(d.pop("sha256", UNSET))

        def _parse_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        path = _parse_path(d.pop("path", UNSET))

        created_at = d.pop("created_at", UNSET)

        message_attachment = cls(
            id=id,
            kind=kind,
            name=name,
            mime=mime,
            size=size,
            sha256=sha256,
            path=path,
            created_at=created_at,
        )

        message_attachment.additional_properties = d
        return message_attachment

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
