from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.gateway_endpoint import GatewayEndpoint, check_gateway_endpoint
from ..models.scope import Scope, check_scope
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.rate_limits import RateLimits


T = TypeVar("T", bound="TokenInfo")


@_attrs_define
class TokenInfo:
    """What lists and the API show about a named token.

    Attributes:
        id (str):
        name (str):
        scopes (list[Scope]):
        created_at (int):
        models (list[str] | Unset): Model alias patterns (`*`, `?`); empty = every model.
        endpoints (list[GatewayEndpoint] | Unset): Gateway endpoint groups; empty = every endpoint.
        limits (RateLimits | Unset): Rate limits of one token (`None` = unlimited).
    """

    id: str
    name: str
    scopes: list[Scope]
    created_at: int
    models: list[str] | Unset = UNSET
    endpoints: list[GatewayEndpoint] | Unset = UNSET
    limits: RateLimits | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        scopes = []
        for scopes_item_data in self.scopes:
            scopes_item: str = scopes_item_data
            scopes.append(scopes_item)

        created_at = self.created_at

        models: list[str] | Unset = UNSET
        if not isinstance(self.models, Unset):
            models = self.models

        endpoints: list[str] | Unset = UNSET
        if not isinstance(self.endpoints, Unset):
            endpoints = []
            for endpoints_item_data in self.endpoints:
                endpoints_item: str = endpoints_item_data
                endpoints.append(endpoints_item)

        limits: dict[str, Any] | Unset = UNSET
        if not isinstance(self.limits, Unset):
            limits = self.limits.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "scopes": scopes,
                "created_at": created_at,
            }
        )
        if models is not UNSET:
            field_dict["models"] = models
        if endpoints is not UNSET:
            field_dict["endpoints"] = endpoints
        if limits is not UNSET:
            field_dict["limits"] = limits

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.rate_limits import RateLimits  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        scopes = []
        _scopes = d.pop("scopes")
        for scopes_item_data in _scopes:
            scopes_item = check_scope(scopes_item_data)

            scopes.append(scopes_item)

        created_at = d.pop("created_at")

        models = cast(list[str], d.pop("models", UNSET))

        _endpoints = d.pop("endpoints", UNSET)
        endpoints: list[GatewayEndpoint] | Unset = UNSET
        if _endpoints is not UNSET:
            endpoints = []
            for endpoints_item_data in _endpoints:
                endpoints_item = check_gateway_endpoint(endpoints_item_data)

                endpoints.append(endpoints_item)

        _limits = d.pop("limits", UNSET)
        limits: RateLimits | Unset
        if isinstance(_limits, Unset):
            limits = UNSET
        else:
            limits = RateLimits.from_dict(_limits)

        token_info = cls(
            id=id,
            name=name,
            scopes=scopes,
            created_at=created_at,
            models=models,
            endpoints=endpoints,
            limits=limits,
        )

        token_info.additional_properties = d
        return token_info

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
