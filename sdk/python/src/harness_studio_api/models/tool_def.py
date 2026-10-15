from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.permission_class import PermissionClass, check_permission_class
from ..models.tool_source import ToolSource, check_tool_source

if TYPE_CHECKING:
    from ..models.tool_def_input_schema import ToolDefInputSchema


T = TypeVar("T", bound="ToolDef")


@_attrs_define
class ToolDef:
    """A tool as the model and the clients see it.

    Attributes:
        name (str):
        description (str):
        input_schema (ToolDefInputSchema): JSON Schema (`type: object`) of the arguments: the `parameters` of an OpenAI
            function.
        class_ (PermissionClass): What a tool may do to the user's machine. The class is fixed by the tool's author (the
            registry entry), never by the model or by call arguments: nothing a model sends can make a
            tool less restricted than it was registered.
        source (ToolSource): Where a tool comes from.
        uses_workspace (bool): The tool works inside a working directory (file tools, shell).
    """

    name: str
    description: str
    input_schema: ToolDefInputSchema
    class_: PermissionClass
    source: ToolSource
    uses_workspace: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        description = self.description

        input_schema = self.input_schema.to_dict()

        class_: str = self.class_

        source: str = self.source

        uses_workspace = self.uses_workspace

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "description": description,
                "input_schema": input_schema,
                "class": class_,
                "source": source,
                "uses_workspace": uses_workspace,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tool_def_input_schema import ToolDefInputSchema  # noqa: PLC0415

        d = dict(src_dict)
        name = d.pop("name")

        description = d.pop("description")

        input_schema = ToolDefInputSchema.from_dict(d.pop("input_schema"))

        class_ = check_permission_class(d.pop("class"))

        source = check_tool_source(d.pop("source"))

        uses_workspace = d.pop("uses_workspace")

        tool_def = cls(
            name=name,
            description=description,
            input_schema=input_schema,
            class_=class_,
            source=source,
            uses_workspace=uses_workspace,
        )

        tool_def.additional_properties = d
        return tool_def

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
