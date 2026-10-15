from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.direct_client import DirectClient
    from ..models.direct_day import DirectDay
    from ..models.direct_server import DirectServer


T = TypeVar("T", bound="DirectUsage")


@_attrs_define
class DirectUsage:
    """Direct usage in the range of an analytics summary: aggregate token totals only, no
    per-request detail. See the module docs for how it is derived.

        Attributes:
            prompt_tokens (int | Unset):  Default: 0.
            completion_tokens (int | Unset):  Default: 0.
            servers (list[DirectServer] | Unset): Per server run, most tokens first.
            daily (list[DirectDay] | Unset):
            clients (list[DirectClient] | Unset):
    """

    prompt_tokens: int | Unset = 0
    completion_tokens: int | Unset = 0
    servers: list[DirectServer] | Unset = UNSET
    daily: list[DirectDay] | Unset = UNSET
    clients: list[DirectClient] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        prompt_tokens = self.prompt_tokens

        completion_tokens = self.completion_tokens

        servers: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.servers, Unset):
            servers = []
            for servers_item_data in self.servers:
                servers_item = servers_item_data.to_dict()
                servers.append(servers_item)

        daily: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.daily, Unset):
            daily = []
            for daily_item_data in self.daily:
                daily_item = daily_item_data.to_dict()
                daily.append(daily_item)

        clients: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.clients, Unset):
            clients = []
            for clients_item_data in self.clients:
                clients_item = clients_item_data.to_dict()
                clients.append(clients_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if prompt_tokens is not UNSET:
            field_dict["prompt_tokens"] = prompt_tokens
        if completion_tokens is not UNSET:
            field_dict["completion_tokens"] = completion_tokens
        if servers is not UNSET:
            field_dict["servers"] = servers
        if daily is not UNSET:
            field_dict["daily"] = daily
        if clients is not UNSET:
            field_dict["clients"] = clients

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.direct_client import DirectClient  # noqa: PLC0415
        from ..models.direct_day import DirectDay  # noqa: PLC0415
        from ..models.direct_server import DirectServer  # noqa: PLC0415

        d = dict(src_dict)
        prompt_tokens = d.pop("prompt_tokens", UNSET)

        completion_tokens = d.pop("completion_tokens", UNSET)

        _servers = d.pop("servers", UNSET)
        servers: list[DirectServer] | Unset = UNSET
        if _servers is not UNSET:
            servers = []
            for servers_item_data in _servers:
                servers_item = DirectServer.from_dict(servers_item_data)

                servers.append(servers_item)

        _daily = d.pop("daily", UNSET)
        daily: list[DirectDay] | Unset = UNSET
        if _daily is not UNSET:
            daily = []
            for daily_item_data in _daily:
                daily_item = DirectDay.from_dict(daily_item_data)

                daily.append(daily_item)

        _clients = d.pop("clients", UNSET)
        clients: list[DirectClient] | Unset = UNSET
        if _clients is not UNSET:
            clients = []
            for clients_item_data in _clients:
                clients_item = DirectClient.from_dict(clients_item_data)

                clients.append(clients_item)

        direct_usage = cls(
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            servers=servers,
            daily=daily,
            clients=clients,
        )

        direct_usage.additional_properties = d
        return direct_usage

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
