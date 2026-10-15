from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.instance_kind import InstanceKind, check_instance_kind

T = TypeVar("T", bound="InstanceSummary")


@_attrs_define
class InstanceSummary:
    """
    Attributes:
        kind (InstanceKind): Which program owns the data directory.
        pid (int):
    """

    kind: InstanceKind
    pid: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind: str = self.kind

        pid = self.pid

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "pid": pid,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        kind = check_instance_kind(d.pop("kind"))

        pid = d.pop("pid")

        instance_summary = cls(
            kind=kind,
            pid=pid,
        )

        instance_summary.additional_properties = d
        return instance_summary

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
