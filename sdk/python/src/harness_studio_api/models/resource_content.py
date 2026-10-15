from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ResourceContent")


@_attrs_define
class ResourceContent:
    """One piece of a resource read (`resources/read` returns a list of them).

    Attributes:
        uri (str):
        mime_type (None | str | Unset):
        text (None | str | Unset): Text contents.
        blob (None | str | Unset): Binary contents, base64.
    """

    uri: str
    mime_type: None | str | Unset = UNSET
    text: None | str | Unset = UNSET
    blob: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        uri = self.uri

        mime_type: None | str | Unset
        if isinstance(self.mime_type, Unset):
            mime_type = UNSET
        else:
            mime_type = self.mime_type

        text: None | str | Unset
        if isinstance(self.text, Unset):
            text = UNSET
        else:
            text = self.text

        blob: None | str | Unset
        if isinstance(self.blob, Unset):
            blob = UNSET
        else:
            blob = self.blob

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "uri": uri,
            }
        )
        if mime_type is not UNSET:
            field_dict["mime_type"] = mime_type
        if text is not UNSET:
            field_dict["text"] = text
        if blob is not UNSET:
            field_dict["blob"] = blob

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        uri = d.pop("uri")

        def _parse_mime_type(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mime_type = _parse_mime_type(d.pop("mime_type", UNSET))

        def _parse_text(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        text = _parse_text(d.pop("text", UNSET))

        def _parse_blob(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        blob = _parse_blob(d.pop("blob", UNSET))

        resource_content = cls(
            uri=uri,
            mime_type=mime_type,
            text=text,
            blob=blob,
        )

        resource_content.additional_properties = d
        return resource_content

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
