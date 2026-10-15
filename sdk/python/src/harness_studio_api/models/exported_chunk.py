from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ExportedChunk")


@_attrs_define
class ExportedChunk:
    """
    Attributes:
        text (str | Unset):  Default: ''.
        page_start (int | None | Unset):
        page_end (int | None | Unset):
        heading (None | str | Unset):
    """

    text: str | Unset = ""
    page_start: int | None | Unset = UNSET
    page_end: int | None | Unset = UNSET
    heading: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        text = self.text

        page_start: int | None | Unset
        if isinstance(self.page_start, Unset):
            page_start = UNSET
        else:
            page_start = self.page_start

        page_end: int | None | Unset
        if isinstance(self.page_end, Unset):
            page_end = UNSET
        else:
            page_end = self.page_end

        heading: None | str | Unset
        if isinstance(self.heading, Unset):
            heading = UNSET
        else:
            heading = self.heading

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if text is not UNSET:
            field_dict["text"] = text
        if page_start is not UNSET:
            field_dict["page_start"] = page_start
        if page_end is not UNSET:
            field_dict["page_end"] = page_end
        if heading is not UNSET:
            field_dict["heading"] = heading

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        text = d.pop("text", UNSET)

        def _parse_page_start(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        page_start = _parse_page_start(d.pop("page_start", UNSET))

        def _parse_page_end(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        page_end = _parse_page_end(d.pop("page_end", UNSET))

        def _parse_heading(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        heading = _parse_heading(d.pop("heading", UNSET))

        exported_chunk = cls(
            text=text,
            page_start=page_start,
            page_end=page_end,
            heading=heading,
        )

        exported_chunk.additional_properties = d
        return exported_chunk

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
