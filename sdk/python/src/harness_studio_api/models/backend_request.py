from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="BackendRequest")


@_attrs_define
class BackendRequest:
    """
    Attributes:
        recipe (str): `llamacpp`, `flm`, `whispercpp`...
        backend (str): `vulkan`, `rocm`, `cpu`, `npu`...
    """

    recipe: str
    backend: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        recipe = self.recipe

        backend = self.backend

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "recipe": recipe,
                "backend": backend,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        recipe = d.pop("recipe")

        backend = d.pop("backend")

        backend_request = cls(
            recipe=recipe,
            backend=backend,
        )

        backend_request.additional_properties = d
        return backend_request

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
