from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="GatewaySettingsTextToolCalls")


@_attrs_define
class GatewaySettingsTextToolCalls:
    """Recover calls left in Chat Completions text, per definition. On by default: only an
    explicit `false` (the opt-out) is stored; a definition without an entry recovers. A
    request that offers no tools is never touched.

    """

    additional_properties: dict[str, bool] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        gateway_settings_text_tool_calls = cls()

        gateway_settings_text_tool_calls.additional_properties = d
        return gateway_settings_text_tool_calls

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> bool:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: bool) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
