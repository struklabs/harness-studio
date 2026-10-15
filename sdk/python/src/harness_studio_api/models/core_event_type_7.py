from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_7_type import (
    CoreEventType7Type,
    check_core_event_type_7_type,
)

T = TypeVar("T", bound="CoreEventType7")


@_attrs_define
class CoreEventType7:
    """A llama-bench run progressed.

    Attributes:
        run_id (str):
        current (int):
        total (int):
        phase (str):
        type_ (CoreEventType7Type):
    """

    run_id: str
    current: int
    total: int
    phase: str
    type_: CoreEventType7Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        run_id = self.run_id

        current = self.current

        total = self.total

        phase = self.phase

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "run_id": run_id,
                "current": current,
                "total": total,
                "phase": phase,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        run_id = d.pop("run_id")

        current = d.pop("current")

        total = d.pop("total")

        phase = d.pop("phase")

        type_ = check_core_event_type_7_type(d.pop("type"))

        core_event_type_7 = cls(
            run_id=run_id,
            current=current,
            total=total,
            phase=phase,
            type_=type_,
        )

        core_event_type_7.additional_properties = d
        return core_event_type_7

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
