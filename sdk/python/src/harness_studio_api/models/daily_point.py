from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="DailyPoint")


@_attrs_define
class DailyPoint:
    """
    Attributes:
        day_start_ms (int): Start of the (UTC) day, Unix ms.
        requests (int):
        errors (int):
        prompt_tokens (int):
        completion_tokens (int):
    """

    day_start_ms: int
    requests: int
    errors: int
    prompt_tokens: int
    completion_tokens: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        day_start_ms = self.day_start_ms

        requests = self.requests

        errors = self.errors

        prompt_tokens = self.prompt_tokens

        completion_tokens = self.completion_tokens

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "day_start_ms": day_start_ms,
                "requests": requests,
                "errors": errors,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        day_start_ms = d.pop("day_start_ms")

        requests = d.pop("requests")

        errors = d.pop("errors")

        prompt_tokens = d.pop("prompt_tokens")

        completion_tokens = d.pop("completion_tokens")

        daily_point = cls(
            day_start_ms=day_start_ms,
            requests=requests,
            errors=errors,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
        )

        daily_point.additional_properties = d
        return daily_point

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
