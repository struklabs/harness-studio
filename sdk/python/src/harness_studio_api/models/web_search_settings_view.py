from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.search_provider_kind import SearchProviderKind, check_search_provider_kind

T = TypeVar("T", bound="WebSearchSettingsView")


@_attrs_define
class WebSearchSettingsView:
    """
    Attributes:
        provider (SearchProviderKind):
        searxng_url (str):
        api_key_set (bool):
    """

    provider: SearchProviderKind
    searxng_url: str
    api_key_set: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        provider: str = self.provider

        searxng_url = self.searxng_url

        api_key_set = self.api_key_set

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "provider": provider,
                "searxng_url": searxng_url,
                "api_key_set": api_key_set,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        provider = check_search_provider_kind(d.pop("provider"))

        searxng_url = d.pop("searxng_url")

        api_key_set = d.pop("api_key_set")

        web_search_settings_view = cls(
            provider=provider,
            searxng_url=searxng_url,
            api_key_set=api_key_set,
        )

        web_search_settings_view.additional_properties = d
        return web_search_settings_view

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
