from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.runtime_source_type_1_kind import (
    RuntimeSourceType1Kind,
    check_runtime_source_type_1_kind,
)

T = TypeVar("T", bound="RuntimeSourceType1")


@_attrs_define
class RuntimeSourceType1:
    """A folder the user picked. `copied` = copied under the data dir at add time; otherwise
    the folder is used in place (never modified or deleted by Harness).

        Attributes:
            path (str):
            copied (bool):
            kind (RuntimeSourceType1Kind):
    """

    path: str
    copied: bool
    kind: RuntimeSourceType1Kind
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        path = self.path

        copied = self.copied

        kind: str = self.kind

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "path": path,
                "copied": copied,
                "kind": kind,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        path = d.pop("path")

        copied = d.pop("copied")

        kind = check_runtime_source_type_1_kind(d.pop("kind"))

        runtime_source_type_1 = cls(
            path=path,
            copied=copied,
            kind=kind,
        )

        runtime_source_type_1.additional_properties = d
        return runtime_source_type_1

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
