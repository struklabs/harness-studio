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


T = TypeVar("T", bound="TokenUpdate")


@_attrs_define
class TokenUpdate:
    """A change to a named token (`None` = keep). The secret never changes; to rotate it,
    create a new token and revoke the old one.

        Attributes:
            name (None | str | Unset):
            scopes (list[Scope] | None | Unset):
            models (list[str] | None | Unset): Model alias patterns (`*`, `?`); `[]` = every model.
            endpoints (list[GatewayEndpoint] | None | Unset): Gateway endpoint groups; `[]` = every endpoint.
            limits (None | RateLimits | Unset):
    """

    name: None | str | Unset = UNSET
    scopes: list[Scope] | None | Unset = UNSET
    models: list[str] | None | Unset = UNSET
    endpoints: list[GatewayEndpoint] | None | Unset = UNSET
    limits: None | RateLimits | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.rate_limits import RateLimits  # noqa: PLC0415

        name: None | str | Unset
        if isinstance(self.name, Unset):
            name = UNSET
        else:
            name = self.name

        scopes: list[str] | None | Unset
        if isinstance(self.scopes, Unset):
            scopes = UNSET
        elif isinstance(self.scopes, list):
            scopes = []
            for scopes_type_0_item_data in self.scopes:
                scopes_type_0_item: str = scopes_type_0_item_data
                scopes.append(scopes_type_0_item)

        else:
            scopes = self.scopes

        models: list[str] | None | Unset
        if isinstance(self.models, Unset):
            models = UNSET
        elif isinstance(self.models, list):
            models = self.models

        else:
            models = self.models

        endpoints: list[str] | None | Unset
        if isinstance(self.endpoints, Unset):
            endpoints = UNSET
        elif isinstance(self.endpoints, list):
            endpoints = []
            for endpoints_type_0_item_data in self.endpoints:
                endpoints_type_0_item: str = endpoints_type_0_item_data
                endpoints.append(endpoints_type_0_item)

        else:
            endpoints = self.endpoints

        limits: dict[str, Any] | None | Unset
        if isinstance(self.limits, Unset):
            limits = UNSET
        elif isinstance(self.limits, RateLimits):
            limits = self.limits.to_dict()
        else:
            limits = self.limits

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if name is not UNSET:
            field_dict["name"] = name
        if scopes is not UNSET:
            field_dict["scopes"] = scopes
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

        def _parse_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        name = _parse_name(d.pop("name", UNSET))

        def _parse_scopes(data: object) -> list[Scope] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                scopes_type_0 = []
                _scopes_type_0 = data
                for scopes_type_0_item_data in _scopes_type_0:
                    scopes_type_0_item = check_scope(scopes_type_0_item_data)

                    scopes_type_0.append(scopes_type_0_item)

                return scopes_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[Scope] | None | Unset, data)

        scopes = _parse_scopes(d.pop("scopes", UNSET))

        def _parse_models(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                models_type_0 = cast(list[str], data)

                return models_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        models = _parse_models(d.pop("models", UNSET))

        def _parse_endpoints(data: object) -> list[GatewayEndpoint] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                endpoints_type_0 = []
                _endpoints_type_0 = data
                for endpoints_type_0_item_data in _endpoints_type_0:
                    endpoints_type_0_item = check_gateway_endpoint(endpoints_type_0_item_data)

                    endpoints_type_0.append(endpoints_type_0_item)

                return endpoints_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[GatewayEndpoint] | None | Unset, data)

        endpoints = _parse_endpoints(d.pop("endpoints", UNSET))

        def _parse_limits(data: object) -> None | RateLimits | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                limits_type_1 = RateLimits.from_dict(data)

                return limits_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RateLimits | Unset, data)

        limits = _parse_limits(d.pop("limits", UNSET))

        token_update = cls(
            name=name,
            scopes=scopes,
            models=models,
            endpoints=endpoints,
            limits=limits,
        )

        token_update.additional_properties = d
        return token_update

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
