from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReasoningSettings")


@_attrs_define
class ReasoningSettings:
    """Reasoning ("thinking") request settings.

    Attributes:
        enabled (bool | None | Unset): `chat_template_kwargs.enable_thinking` (templates that support it, e.g. Qwen3).
        format_ (None | str | Unset): `reasoning_format`: none | deepseek | deepseek-legacy | auto.
        effort (None | str | Unset): `reasoning_effort` (gpt-oss): low | medium | high.
        budget (int | None | Unset): `thinking_budget_tokens` (-1 = unlimited, 0 = off).
    """

    enabled: bool | None | Unset = UNSET
    format_: None | str | Unset = UNSET
    effort: None | str | Unset = UNSET
    budget: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        enabled: bool | None | Unset
        if isinstance(self.enabled, Unset):
            enabled = UNSET
        else:
            enabled = self.enabled

        format_: None | str | Unset
        if isinstance(self.format_, Unset):
            format_ = UNSET
        else:
            format_ = self.format_

        effort: None | str | Unset
        if isinstance(self.effort, Unset):
            effort = UNSET
        else:
            effort = self.effort

        budget: int | None | Unset
        if isinstance(self.budget, Unset):
            budget = UNSET
        else:
            budget = self.budget

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if format_ is not UNSET:
            field_dict["format"] = format_
        if effort is not UNSET:
            field_dict["effort"] = effort
        if budget is not UNSET:
            field_dict["budget"] = budget

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_enabled(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        enabled = _parse_enabled(d.pop("enabled", UNSET))

        def _parse_format_(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        format_ = _parse_format_(d.pop("format", UNSET))

        def _parse_effort(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        effort = _parse_effort(d.pop("effort", UNSET))

        def _parse_budget(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        budget = _parse_budget(d.pop("budget", UNSET))

        reasoning_settings = cls(
            enabled=enabled,
            format_=format_,
            effort=effort,
            budget=budget,
        )

        reasoning_settings.additional_properties = d
        return reasoning_settings

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
