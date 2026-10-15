from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.client_endpoint import ClientEndpoint
    from ..models.port_fallback import PortFallback


T = TypeVar("T", bound="GatewayStatus")


@_attrs_define
class GatewayStatus:
    """What the gateway listener is doing right now (process-wide; set by the HTTP side).

    Attributes:
        running (bool):
        require_token (bool):
        host (None | str | Unset):
        port (int | None | Unset):
        endpoint (ClientEndpoint | None | Unset):
        error (None | str | Unset): Why the last start failed (i18n key or text).
        started_at (int | None | Unset): Unix seconds of the start.
        ollama_port (int | None | Unset): The Ollama port the gateway also listens on (`ollama_port_enabled`), when
            bound.
        ollama_warning (None | str | Unset): Why the Ollama port is not served (i18n key `WARN_OLLAMA_*`, `: <port>`
            appended).
        port_fallback (None | PortFallback | Unset):
        clients_moved (list[str] | Unset): Client configs Harness moved to this port at the start (display names).
        clients_pending (list[str] | Unset): Client configs still on another address after that (a key in the file, a
            server's own
            port): their card shows the warning with "Düzelt".
    """

    running: bool
    require_token: bool
    host: None | str | Unset = UNSET
    port: int | None | Unset = UNSET
    endpoint: ClientEndpoint | None | Unset = UNSET
    error: None | str | Unset = UNSET
    started_at: int | None | Unset = UNSET
    ollama_port: int | None | Unset = UNSET
    ollama_warning: None | str | Unset = UNSET
    port_fallback: None | PortFallback | Unset = UNSET
    clients_moved: list[str] | Unset = UNSET
    clients_pending: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.client_endpoint import ClientEndpoint  # noqa: PLC0415
        from ..models.port_fallback import PortFallback  # noqa: PLC0415

        running = self.running

        require_token = self.require_token

        host: None | str | Unset
        if isinstance(self.host, Unset):
            host = UNSET
        else:
            host = self.host

        port: int | None | Unset
        if isinstance(self.port, Unset):
            port = UNSET
        else:
            port = self.port

        endpoint: dict[str, Any] | None | Unset
        if isinstance(self.endpoint, Unset):
            endpoint = UNSET
        elif isinstance(self.endpoint, ClientEndpoint):
            endpoint = self.endpoint.to_dict()
        else:
            endpoint = self.endpoint

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        started_at: int | None | Unset
        if isinstance(self.started_at, Unset):
            started_at = UNSET
        else:
            started_at = self.started_at

        ollama_port: int | None | Unset
        if isinstance(self.ollama_port, Unset):
            ollama_port = UNSET
        else:
            ollama_port = self.ollama_port

        ollama_warning: None | str | Unset
        if isinstance(self.ollama_warning, Unset):
            ollama_warning = UNSET
        else:
            ollama_warning = self.ollama_warning

        port_fallback: dict[str, Any] | None | Unset
        if isinstance(self.port_fallback, Unset):
            port_fallback = UNSET
        elif isinstance(self.port_fallback, PortFallback):
            port_fallback = self.port_fallback.to_dict()
        else:
            port_fallback = self.port_fallback

        clients_moved: list[str] | Unset = UNSET
        if not isinstance(self.clients_moved, Unset):
            clients_moved = self.clients_moved

        clients_pending: list[str] | Unset = UNSET
        if not isinstance(self.clients_pending, Unset):
            clients_pending = self.clients_pending

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "running": running,
                "require_token": require_token,
            }
        )
        if host is not UNSET:
            field_dict["host"] = host
        if port is not UNSET:
            field_dict["port"] = port
        if endpoint is not UNSET:
            field_dict["endpoint"] = endpoint
        if error is not UNSET:
            field_dict["error"] = error
        if started_at is not UNSET:
            field_dict["started_at"] = started_at
        if ollama_port is not UNSET:
            field_dict["ollama_port"] = ollama_port
        if ollama_warning is not UNSET:
            field_dict["ollama_warning"] = ollama_warning
        if port_fallback is not UNSET:
            field_dict["port_fallback"] = port_fallback
        if clients_moved is not UNSET:
            field_dict["clients_moved"] = clients_moved
        if clients_pending is not UNSET:
            field_dict["clients_pending"] = clients_pending

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.client_endpoint import ClientEndpoint  # noqa: PLC0415
        from ..models.port_fallback import PortFallback  # noqa: PLC0415

        d = dict(src_dict)
        running = d.pop("running")

        require_token = d.pop("require_token")

        def _parse_host(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        host = _parse_host(d.pop("host", UNSET))

        def _parse_port(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        port = _parse_port(d.pop("port", UNSET))

        def _parse_endpoint(data: object) -> ClientEndpoint | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                endpoint_type_1 = ClientEndpoint.from_dict(data)

                return endpoint_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ClientEndpoint | None | Unset, data)

        endpoint = _parse_endpoint(d.pop("endpoint", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_started_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        started_at = _parse_started_at(d.pop("started_at", UNSET))

        def _parse_ollama_port(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ollama_port = _parse_ollama_port(d.pop("ollama_port", UNSET))

        def _parse_ollama_warning(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        ollama_warning = _parse_ollama_warning(d.pop("ollama_warning", UNSET))

        def _parse_port_fallback(data: object) -> None | PortFallback | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                port_fallback_type_1 = PortFallback.from_dict(data)

                return port_fallback_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | PortFallback | Unset, data)

        port_fallback = _parse_port_fallback(d.pop("port_fallback", UNSET))

        clients_moved = cast(list[str], d.pop("clients_moved", UNSET))

        clients_pending = cast(list[str], d.pop("clients_pending", UNSET))

        gateway_status = cls(
            running=running,
            require_token=require_token,
            host=host,
            port=port,
            endpoint=endpoint,
            error=error,
            started_at=started_at,
            ollama_port=ollama_port,
            ollama_warning=ollama_warning,
            port_fallback=port_fallback,
            clients_moved=clients_moved,
            clients_pending=clients_pending,
        )

        gateway_status.additional_properties = d
        return gateway_status

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
