from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.tool_source import ToolSource, check_tool_source
from ..types import UNSET, Unset

T = TypeVar("T", bound="InventoryTool")


@_attrs_define
class InventoryTool:
    """
    Attributes:
        name (str):
        description (str):
        category (str):
        source (ToolSource): Where a tool comes from.
        server (None | str | Unset):
    """

    name: str
    description: str
    category: str
    source: ToolSource
    server: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        description = self.description

        category = self.category

        source: str = self.source

        server: None | str | Unset
        if isinstance(self.server, Unset):
            server = UNSET
        else:
            server = self.server

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "description": description,
                "category": category,
                "source": source,
            }
        )
        if server is not UNSET:
            field_dict["server"] = server

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        description = d.pop("description")

        category = d.pop("category")

        source = check_tool_source(d.pop("source"))

        def _parse_server(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        server = _parse_server(d.pop("server", UNSET))

        inventory_tool = cls(
            name=name,
            description=description,
            category=category,
            source=source,
            server=server,
        )

        inventory_tool.additional_properties = d
        return inventory_tool

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
