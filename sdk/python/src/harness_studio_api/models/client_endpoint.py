from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.endpoint_kind import EndpointKind, check_endpoint_kind

T = TypeVar("T", bound="ClientEndpoint")


@_attrs_define
class ClientEndpoint:
    """An OpenAI/Anthropic-compatible address for clients.

    Attributes:
        kind (EndpointKind): Which kind of endpoint a client talks to.
        host_url (str): `http://host:port` (no path).
        base_url (str): `{host_url}/v1`.
        requires_token (bool): Requests need a token (managed: gateway setting; direct: the server has an API key).
    """

    kind: EndpointKind
    host_url: str
    base_url: str
    requires_token: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind: str = self.kind

        host_url = self.host_url

        base_url = self.base_url

        requires_token = self.requires_token

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "host_url": host_url,
                "base_url": base_url,
                "requires_token": requires_token,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        kind = check_endpoint_kind(d.pop("kind"))

        host_url = d.pop("host_url")

        base_url = d.pop("base_url")

        requires_token = d.pop("requires_token")

        client_endpoint = cls(
            kind=kind,
            host_url=host_url,
            base_url=base_url,
            requires_token=requires_token,
        )

        client_endpoint.additional_properties = d
        return client_endpoint

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
