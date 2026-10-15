from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ToolLimits")


@_attrs_define
class ToolLimits:
    """The limits as clients see them (`GET /tools`, the Playground's `tools_list`).

    Attributes:
        max_rounds (int): Tool rounds an agent loop runs before it stops and says so.
        timeout_secs (int): Seconds one call may take (a tool may bound itself tighter).
        max_result_chars (int): A result longer than this many characters is cut and the cut is marked.
    """

    max_rounds: int
    timeout_secs: int
    max_result_chars: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        max_rounds = self.max_rounds

        timeout_secs = self.timeout_secs

        max_result_chars = self.max_result_chars

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "max_rounds": max_rounds,
                "timeout_secs": timeout_secs,
                "max_result_chars": max_result_chars,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        max_rounds = d.pop("max_rounds")

        timeout_secs = d.pop("timeout_secs")

        max_result_chars = d.pop("max_result_chars")

        tool_limits = cls(
            max_rounds=max_rounds,
            timeout_secs=timeout_secs,
            max_result_chars=max_result_chars,
        )

        tool_limits.additional_properties = d
        return tool_limits

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
