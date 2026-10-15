from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.model_ref_type_0_kind import (
    ModelRefType0Kind,
    check_model_ref_type_0_kind,
)

T = TypeVar("T", bound="ModelRefType0")


@_attrs_define
class ModelRefType0:
    """A local GGUF (`LocalModel.id`: canonical launch path; shard 1 for split models).

    Attributes:
        path (str):
        kind (ModelRefType0Kind):
    """

    path: str
    kind: ModelRefType0Kind
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        path = self.path

        kind: str = self.kind

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "path": path,
                "kind": kind,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        path = d.pop("path")

        kind = check_model_ref_type_0_kind(d.pop("kind"))

        model_ref_type_0 = cls(
            path=path,
            kind=kind,
        )

        model_ref_type_0.additional_properties = d
        return model_ref_type_0

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
