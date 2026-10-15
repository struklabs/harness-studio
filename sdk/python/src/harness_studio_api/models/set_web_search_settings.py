from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.search_provider_kind import SearchProviderKind, check_search_provider_kind
from ..types import UNSET, Unset

T = TypeVar("T", bound="SetWebSearchSettings")


@_attrs_define
class SetWebSearchSettings:
    """
    Attributes:
        provider (SearchProviderKind):
        searxng_url (str | Unset):
        api_key (None | str | Unset): Missing keeps the current key; empty removes it. Never serialized to
            settings.json.
    """

    provider: SearchProviderKind
    searxng_url: str | Unset = UNSET
    api_key: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        provider: str = self.provider

        searxng_url = self.searxng_url

        api_key: None | str | Unset
        if isinstance(self.api_key, Unset):
            api_key = UNSET
        else:
            api_key = self.api_key

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "provider": provider,
            }
        )
        if searxng_url is not UNSET:
            field_dict["searxng_url"] = searxng_url
        if api_key is not UNSET:
            field_dict["api_key"] = api_key

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        provider = check_search_provider_kind(d.pop("provider"))

        searxng_url = d.pop("searxng_url", UNSET)

        def _parse_api_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        api_key = _parse_api_key(d.pop("api_key", UNSET))

        set_web_search_settings = cls(
            provider=provider,
            searxng_url=searxng_url,
            api_key=api_key,
        )

        set_web_search_settings.additional_properties = d
        return set_web_search_settings

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
