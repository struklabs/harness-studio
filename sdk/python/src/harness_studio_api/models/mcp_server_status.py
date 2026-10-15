from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.mcp_auth_state import McpAuthState, check_mcp_auth_state
from ..models.mcp_server_state import McpServerState, check_mcp_server_state
from ..types import UNSET, Unset

T = TypeVar("T", bound="McpServerStatus")


@_attrs_define
class McpServerStatus:
    """What `mcp-client status` shows for one configured server.

    Attributes:
        name (str):
        transport (str): `stdio`, `http` (Streamable HTTP) or `sse` (the older HTTP+SSE transport).
        state (McpServerState):
        tools (int): Number of tools registered.
        error (None | str | Unset):
        protocol (None | str | Unset): Negotiated MCP version (`2026-07-28`, `2025-11-25`...).
        server (None | str | Unset): `serverInfo` as the server reports it (display only).
        auth (McpAuthState | None | Unset):
        resources (bool | Unset): The connected server offers resources.
    """

    name: str
    transport: str
    state: McpServerState
    tools: int
    error: None | str | Unset = UNSET
    protocol: None | str | Unset = UNSET
    server: None | str | Unset = UNSET
    auth: McpAuthState | None | Unset = UNSET
    resources: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        transport = self.transport

        state: str = self.state

        tools = self.tools

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        protocol: None | str | Unset
        if isinstance(self.protocol, Unset):
            protocol = UNSET
        else:
            protocol = self.protocol

        server: None | str | Unset
        if isinstance(self.server, Unset):
            server = UNSET
        else:
            server = self.server

        auth: None | str | Unset
        if isinstance(self.auth, Unset):
            auth = UNSET
        elif isinstance(self.auth, str):
            auth = self.auth
        else:
            auth = self.auth

        resources = self.resources

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "transport": transport,
                "state": state,
                "tools": tools,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error
        if protocol is not UNSET:
            field_dict["protocol"] = protocol
        if server is not UNSET:
            field_dict["server"] = server
        if auth is not UNSET:
            field_dict["auth"] = auth
        if resources is not UNSET:
            field_dict["resources"] = resources

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        name = d.pop("name")

        transport = d.pop("transport")

        state = check_mcp_server_state(d.pop("state"))

        tools = d.pop("tools")

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_protocol(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        protocol = _parse_protocol(d.pop("protocol", UNSET))

        def _parse_server(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        server = _parse_server(d.pop("server", UNSET))

        def _parse_auth(data: object) -> McpAuthState | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                auth_type_1 = check_mcp_auth_state(data)

                return auth_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(McpAuthState | None | Unset, data)

        auth = _parse_auth(d.pop("auth", UNSET))

        resources = d.pop("resources", UNSET)

        mcp_server_status = cls(
            name=name,
            transport=transport,
            state=state,
            tools=tools,
            error=error,
            protocol=protocol,
            server=server,
            auth=auth,
            resources=resources,
        )

        mcp_server_status.additional_properties = d
        return mcp_server_status

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
