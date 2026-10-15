from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.model_ref_type_1_kind import (
    ModelRefType1Kind,
    check_model_ref_type_1_kind,
)

T = TypeVar("T", bound="ModelRefType1")


@_attrs_define
class ModelRefType1:
    """A Hugging Face file not downloaded yet (`hf:<repo>:<file>`).

    Attributes:
        repo (str):
        file (str):
        kind (ModelRefType1Kind):
    """

    repo: str
    file: str
    kind: ModelRefType1Kind
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo = self.repo

        file = self.file

        kind: str = self.kind

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo": repo,
                "file": file,
                "kind": kind,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        repo = d.pop("repo")

        file = d.pop("file")

        kind = check_model_ref_type_1_kind(d.pop("kind"))

        model_ref_type_1 = cls(
            repo=repo,
            file=file,
            kind=kind,
        )

        model_ref_type_1.additional_properties = d
        return model_ref_type_1

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
