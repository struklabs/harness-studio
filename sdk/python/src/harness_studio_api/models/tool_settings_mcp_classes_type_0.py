from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.permission_class import PermissionClass, check_permission_class

T = TypeVar("T", bound="ToolSettingsMcpClassesType0")


@_attrs_define
class ToolSettingsMcpClassesType0:
    """The user's permission class for an MCP tool (by its registry name, `<server>_<tool>`),
    replacing the class its annotations give it. Only MCP tools: a built-in tool's class is
    fixed in the code.

    `None` = not given: saving settings from a screen that does not know the table keeps
    the stored one (`save_settings`); `Some(empty)` clears it.

    """

    additional_properties: dict[str, PermissionClass] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:

        field_dict: dict[str, Any] = {}
        for prop_name, prop in self.additional_properties.items():
            field_dict[prop_name] = prop

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        tool_settings_mcp_classes_type_0 = cls()

        additional_properties = {}
        for prop_name, prop_dict in d.items():
            additional_property = check_permission_class(prop_dict)

            additional_properties[prop_name] = additional_property

        tool_settings_mcp_classes_type_0.additional_properties = additional_properties
        return tool_settings_mcp_classes_type_0

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> PermissionClass:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: PermissionClass) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
