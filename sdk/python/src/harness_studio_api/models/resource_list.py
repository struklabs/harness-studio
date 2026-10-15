from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.remote_resource import RemoteResource
    from ..models.remote_resource_template import RemoteResourceTemplate


T = TypeVar("T", bound="ResourceList")


@_attrs_define
class ResourceList:
    """The resources and templates of one server.

    Attributes:
        resources (list[RemoteResource]):
        templates (list[RemoteResourceTemplate]):
    """

    resources: list[RemoteResource]
    templates: list[RemoteResourceTemplate]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        resources = []
        for resources_item_data in self.resources:
            resources_item = resources_item_data.to_dict()
            resources.append(resources_item)

        templates = []
        for templates_item_data in self.templates:
            templates_item = templates_item_data.to_dict()
            templates.append(templates_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "resources": resources,
                "templates": templates,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.remote_resource import RemoteResource  # noqa: PLC0415
        from ..models.remote_resource_template import (
            RemoteResourceTemplate,  # noqa: PLC0415
        )

        d = dict(src_dict)
        resources = []
        _resources = d.pop("resources")
        for resources_item_data in _resources:
            resources_item = RemoteResource.from_dict(resources_item_data)

            resources.append(resources_item)

        templates = []
        _templates = d.pop("templates")
        for templates_item_data in _templates:
            templates_item = RemoteResourceTemplate.from_dict(templates_item_data)

            templates.append(templates_item)

        resource_list = cls(
            resources=resources,
            templates=templates,
        )

        resource_list.additional_properties = d
        return resource_list

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
