from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.resource_content import ResourceContent


T = TypeVar("T", bound="ResourceRead")


@_attrs_define
class ResourceRead:
    """
    Attributes:
        contents (list[ResourceContent]):
    """

    contents: list[ResourceContent]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        contents = []
        for contents_item_data in self.contents:
            contents_item = contents_item_data.to_dict()
            contents.append(contents_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "contents": contents,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.resource_content import ResourceContent  # noqa: PLC0415

        d = dict(src_dict)
        contents = []
        _contents = d.pop("contents")
        for contents_item_data in _contents:
            contents_item = ResourceContent.from_dict(contents_item_data)

            contents.append(contents_item)

        resource_read = cls(
            contents=contents,
        )

        resource_read.additional_properties = d
        return resource_read

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
