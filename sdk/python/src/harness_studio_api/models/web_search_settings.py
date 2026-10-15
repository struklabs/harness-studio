from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.search_provider_kind import SearchProviderKind, check_search_provider_kind
from ..types import UNSET, Unset

T = TypeVar("T", bound="WebSearchSettings")


@_attrs_define
class WebSearchSettings:
    """
    Attributes:
        provider (SearchProviderKind | Unset):
        searxng_url (str | Unset): SearXNG base URL only. API keys are always held by the OS keychain. Default: ''.
    """

    provider: SearchProviderKind | Unset = UNSET
    searxng_url: str | Unset = ""
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        provider: str | Unset = UNSET
        if not isinstance(self.provider, Unset):
            provider = self.provider

        searxng_url = self.searxng_url

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if provider is not UNSET:
            field_dict["provider"] = provider
        if searxng_url is not UNSET:
            field_dict["searxng_url"] = searxng_url

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        _provider = d.pop("provider", UNSET)
        provider: SearchProviderKind | Unset
        if isinstance(_provider, Unset):
            provider = UNSET
        else:
            provider = check_search_provider_kind(_provider)

        searxng_url = d.pop("searxng_url", UNSET)

        web_search_settings = cls(
            provider=provider,
            searxng_url=searxng_url,
        )

        web_search_settings.additional_properties = d
        return web_search_settings

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
