from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.serve_mode import ServeMode, check_serve_mode
from ..types import UNSET, Unset

T = TypeVar("T", bound="ActiveServerInfo")


@_attrs_define
class ActiveServerInfo:
    """What the UI and telemetry need to talk to the running server.

    Attributes:
        connect_host (str): Host to connect to (wildcard bind addresses mapped to loopback).
        bind_host (str):
        port (int):
        api_key_set (bool):
        model_path (str):
        router_mode (bool):
        api_key_required (bool | Unset): Requests need a key: the user's (`api_key_set`) or one made for this run (a
            Python
            worker always has one). The frontend fetches it with `get_server_api_key`.
        exposure_acknowledged (bool | Unset): Started on a network address without an API key after the user's "open
            anyway".
        cors_origins (None | str | Unset): `--cors-origins` sent to this run (`None` = llama-server's default).
        serve_mode (ServeMode | Unset):
        engine_family (str | Unset): Runtime family serving this run (`llama.cpp`, `mlx`, `mlx-vlm`, `mlx-audio`, ...):
            the
            routes a client can call depend on it. Empty for runs recorded before it existed.
    """

    connect_host: str
    bind_host: str
    port: int
    api_key_set: bool
    model_path: str
    router_mode: bool
    api_key_required: bool | Unset = UNSET
    exposure_acknowledged: bool | Unset = UNSET
    cors_origins: None | str | Unset = UNSET
    serve_mode: ServeMode | Unset = UNSET
    engine_family: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        connect_host = self.connect_host

        bind_host = self.bind_host

        port = self.port

        api_key_set = self.api_key_set

        model_path = self.model_path

        router_mode = self.router_mode

        api_key_required = self.api_key_required

        exposure_acknowledged = self.exposure_acknowledged

        cors_origins: None | str | Unset
        if isinstance(self.cors_origins, Unset):
            cors_origins = UNSET
        else:
            cors_origins = self.cors_origins

        serve_mode: str | Unset = UNSET
        if not isinstance(self.serve_mode, Unset):
            serve_mode = self.serve_mode

        engine_family = self.engine_family

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "connect_host": connect_host,
                "bind_host": bind_host,
                "port": port,
                "api_key_set": api_key_set,
                "model_path": model_path,
                "router_mode": router_mode,
            }
        )
        if api_key_required is not UNSET:
            field_dict["api_key_required"] = api_key_required
        if exposure_acknowledged is not UNSET:
            field_dict["exposure_acknowledged"] = exposure_acknowledged
        if cors_origins is not UNSET:
            field_dict["cors_origins"] = cors_origins
        if serve_mode is not UNSET:
            field_dict["serve_mode"] = serve_mode
        if engine_family is not UNSET:
            field_dict["engine_family"] = engine_family

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        connect_host = d.pop("connect_host")

        bind_host = d.pop("bind_host")

        port = d.pop("port")

        api_key_set = d.pop("api_key_set")

        model_path = d.pop("model_path")

        router_mode = d.pop("router_mode")

        api_key_required = d.pop("api_key_required", UNSET)

        exposure_acknowledged = d.pop("exposure_acknowledged", UNSET)

        def _parse_cors_origins(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cors_origins = _parse_cors_origins(d.pop("cors_origins", UNSET))

        _serve_mode = d.pop("serve_mode", UNSET)
        serve_mode: ServeMode | Unset
        if isinstance(_serve_mode, Unset):
            serve_mode = UNSET
        else:
            serve_mode = check_serve_mode(_serve_mode)

        engine_family = d.pop("engine_family", UNSET)

        active_server_info = cls(
            connect_host=connect_host,
            bind_host=bind_host,
            port=port,
            api_key_set=api_key_set,
            model_path=model_path,
            router_mode=router_mode,
            api_key_required=api_key_required,
            exposure_acknowledged=exposure_acknowledged,
            cors_origins=cors_origins,
            serve_mode=serve_mode,
            engine_family=engine_family,
        )

        active_server_info.additional_properties = d
        return active_server_info

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
