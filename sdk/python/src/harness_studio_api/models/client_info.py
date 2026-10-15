from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ClientInfo")


@_attrs_define
class ClientInfo:
    """Who sent a request.

    Attributes:
        user_agent (None | str | Unset): `User-Agent` header.
        harness_client (None | str | Unset): Optional `X-Harness-Client` header (e.g. `opencode`, `harness-playground`).
        token_name (None | str | Unset): Name of the named token used (`harness` for the gateway's own client token).
        label (None | str | Unset): Best label in this order: `X-Harness-Client`, token name, a known
            `User-Agent` product, else the raw agent.
    """

    user_agent: None | str | Unset = UNSET
    harness_client: None | str | Unset = UNSET
    token_name: None | str | Unset = UNSET
    label: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        user_agent: None | str | Unset
        if isinstance(self.user_agent, Unset):
            user_agent = UNSET
        else:
            user_agent = self.user_agent

        harness_client: None | str | Unset
        if isinstance(self.harness_client, Unset):
            harness_client = UNSET
        else:
            harness_client = self.harness_client

        token_name: None | str | Unset
        if isinstance(self.token_name, Unset):
            token_name = UNSET
        else:
            token_name = self.token_name

        label: None | str | Unset
        if isinstance(self.label, Unset):
            label = UNSET
        else:
            label = self.label

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if user_agent is not UNSET:
            field_dict["user_agent"] = user_agent
        if harness_client is not UNSET:
            field_dict["harness_client"] = harness_client
        if token_name is not UNSET:
            field_dict["token_name"] = token_name
        if label is not UNSET:
            field_dict["label"] = label

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_user_agent(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        user_agent = _parse_user_agent(d.pop("user_agent", UNSET))

        def _parse_harness_client(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        harness_client = _parse_harness_client(d.pop("harness_client", UNSET))

        def _parse_token_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        token_name = _parse_token_name(d.pop("token_name", UNSET))

        def _parse_label(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        label = _parse_label(d.pop("label", UNSET))

        client_info = cls(
            user_agent=user_agent,
            harness_client=harness_client,
            token_name=token_name,
            label=label,
        )

        client_info.additional_properties = d
        return client_info

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
