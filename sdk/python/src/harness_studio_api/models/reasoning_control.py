from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ReasoningControl")


@_attrs_define
class ReasoningControl:
    """
    Attributes:
        effort (bool):
        toggle (bool):
        budget (bool):
    """

    effort: bool
    toggle: bool
    budget: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        effort = self.effort

        toggle = self.toggle

        budget = self.budget

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "effort": effort,
                "toggle": toggle,
                "budget": budget,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        effort = d.pop("effort")

        toggle = d.pop("toggle")

        budget = d.pop("budget")

        reasoning_control = cls(
            effort=effort,
            toggle=toggle,
            budget=budget,
        )

        reasoning_control.additional_properties = d
        return reasoning_control

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
