from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.gateway_settings_server_tools import GatewaySettingsServerTools
    from ..models.gateway_settings_text_tool_calls import GatewaySettingsTextToolCalls
    from ..models.gateway_settings_tool_ignores import GatewaySettingsToolIgnores
    from ..models.responses_settings import ResponsesSettings


T = TypeVar("T", bound="GatewaySettings")


@_attrs_define
class GatewaySettings:
    """`settings.json` → `gateway`. Changed only through [`validate_settings`] (the general
    settings save keeps the stored block, like `control_api`).

        Attributes:
            enabled (bool | Unset): Off by default; the user turns it on (Settings > Gateway) or runs `harness serve
                --gateway`. Default: False.
            host (str | Unset):  Default: '127.0.0.1'.
            port (int | Unset):  Default: 11435.
            allow_network (bool | Unset): A non-loopback bind must be allowed explicitly and always requires a token.
                Default: False.
            require_token (bool | Unset): Require a token on loopback too (a network bind always does). Accepted: named
                tokens
                with the `inference` (or `admin`) scope, and the gateway's own client token. Default: False.
            cors_origins (list[str] | Unset): Browser origins allowed by CORS (the Harness window is always allowed).
            max_loaded_models (int | Unset): Models the gateway keeps loaded at once (0 = no limit; memory decides).
                Default: 0.
            max_queue (int | Unset): Requests that may wait per definition (for a load or a free slot); more → 503 +
                `Retry-After`. Default: 32.
            request_timeout_secs (int | Unset): Seconds a request may wait for a slot and for the upstream response to
                start, and the
                longest pause inside a stream. 0 = no limit. Default: 600.
            default_ttl_secs (int | None | Unset): Idle unload after this many seconds for definitions without their own
                TTL. `None`
                (default) = never.
            memory_aware (bool | Unset): Evict least recently used idle models when the next one does not fit in free VRAM
                (estimate/measurement of `vram.rs`, free memory of `--list-devices`). Default: True.
            memory_headroom_mib (int | Unset): VRAM kept free on top of the estimate, MiB. 0 = automatic
                (`vram::memory_headroom`:
                max(1 GiB, 8% of the card)); a value above 0 is used as given. A stored 512 (the old
                default) loads as 0. Default: 0.
            tool_ignores (GatewaySettingsToolIgnores | Unset): Explicit per-definition consent to ignore tools the local
                server cannot run
                (definition id → tool types or names, e.g. `web_search`). Every ignored tool is
                reported in the `X-Harness-Compat` response header and in the log.
            server_tools (GatewaySettingsServerTools | Unset): Server-side tools, per definition id: the gateway runs the
                tool loop itself for
                these models instead of rejecting hosted tools. Off (empty) by default; see
                [`ServerTools`].
            text_tool_calls (GatewaySettingsTextToolCalls | Unset): Recover calls left in Chat Completions text, per
                definition. On by default: only an
                explicit `false` (the opt-out) is stored; a definition without an entry recovers. A
                request that offers no tools is never touched.
            ollama_port_enabled (bool | Unset): Also listen on the Ollama port (`ollama_port`, default 11434) with the same
                routes, for
                clients that only know Ollama's address. Off by default. When something already
                answers there (a real Ollama), the gateway does not bind it and reports
                `GatewayStatus::ollama_warning` instead; Ollama's default port is also refused while
                Ollama is installed (not running), so a later Ollama start still works. Default: False.
            ollama_port (int | Unset):  Default: 11434.
            ollama_allow_pull (bool | Unset): Let `/api/pull` queue Hugging Face downloads (explicit
                `hf.co/<owner>/<repo>[:quant]`
                references only). Off by default. Default: False.
            responses (ResponsesSettings | Unset): `settings.json` → `gateway.responses`.
    """

    enabled: bool | Unset = False
    host: str | Unset = "127.0.0.1"
    port: int | Unset = 11435
    allow_network: bool | Unset = False
    require_token: bool | Unset = False
    cors_origins: list[str] | Unset = UNSET
    max_loaded_models: int | Unset = 0
    max_queue: int | Unset = 32
    request_timeout_secs: int | Unset = 600
    default_ttl_secs: int | None | Unset = UNSET
    memory_aware: bool | Unset = True
    memory_headroom_mib: int | Unset = 0
    tool_ignores: GatewaySettingsToolIgnores | Unset = UNSET
    server_tools: GatewaySettingsServerTools | Unset = UNSET
    text_tool_calls: GatewaySettingsTextToolCalls | Unset = UNSET
    ollama_port_enabled: bool | Unset = False
    ollama_port: int | Unset = 11434
    ollama_allow_pull: bool | Unset = False
    responses: ResponsesSettings | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        host = self.host

        port = self.port

        allow_network = self.allow_network

        require_token = self.require_token

        cors_origins: list[str] | Unset = UNSET
        if not isinstance(self.cors_origins, Unset):
            cors_origins = self.cors_origins

        max_loaded_models = self.max_loaded_models

        max_queue = self.max_queue

        request_timeout_secs = self.request_timeout_secs

        default_ttl_secs: int | None | Unset
        if isinstance(self.default_ttl_secs, Unset):
            default_ttl_secs = UNSET
        else:
            default_ttl_secs = self.default_ttl_secs

        memory_aware = self.memory_aware

        memory_headroom_mib = self.memory_headroom_mib

        tool_ignores: dict[str, Any] | Unset = UNSET
        if not isinstance(self.tool_ignores, Unset):
            tool_ignores = self.tool_ignores.to_dict()

        server_tools: dict[str, Any] | Unset = UNSET
        if not isinstance(self.server_tools, Unset):
            server_tools = self.server_tools.to_dict()

        text_tool_calls: dict[str, Any] | Unset = UNSET
        if not isinstance(self.text_tool_calls, Unset):
            text_tool_calls = self.text_tool_calls.to_dict()

        ollama_port_enabled = self.ollama_port_enabled

        ollama_port = self.ollama_port

        ollama_allow_pull = self.ollama_allow_pull

        responses: dict[str, Any] | Unset = UNSET
        if not isinstance(self.responses, Unset):
            responses = self.responses.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if host is not UNSET:
            field_dict["host"] = host
        if port is not UNSET:
            field_dict["port"] = port
        if allow_network is not UNSET:
            field_dict["allow_network"] = allow_network
        if require_token is not UNSET:
            field_dict["require_token"] = require_token
        if cors_origins is not UNSET:
            field_dict["cors_origins"] = cors_origins
        if max_loaded_models is not UNSET:
            field_dict["max_loaded_models"] = max_loaded_models
        if max_queue is not UNSET:
            field_dict["max_queue"] = max_queue
        if request_timeout_secs is not UNSET:
            field_dict["request_timeout_secs"] = request_timeout_secs
        if default_ttl_secs is not UNSET:
            field_dict["default_ttl_secs"] = default_ttl_secs
        if memory_aware is not UNSET:
            field_dict["memory_aware"] = memory_aware
        if memory_headroom_mib is not UNSET:
            field_dict["memory_headroom_mib"] = memory_headroom_mib
        if tool_ignores is not UNSET:
            field_dict["tool_ignores"] = tool_ignores
        if server_tools is not UNSET:
            field_dict["server_tools"] = server_tools
        if text_tool_calls is not UNSET:
            field_dict["text_tool_calls"] = text_tool_calls
        if ollama_port_enabled is not UNSET:
            field_dict["ollama_port_enabled"] = ollama_port_enabled
        if ollama_port is not UNSET:
            field_dict["ollama_port"] = ollama_port
        if ollama_allow_pull is not UNSET:
            field_dict["ollama_allow_pull"] = ollama_allow_pull
        if responses is not UNSET:
            field_dict["responses"] = responses

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gateway_settings_server_tools import (
            GatewaySettingsServerTools,  # noqa: PLC0415
        )
        from ..models.gateway_settings_text_tool_calls import (
            GatewaySettingsTextToolCalls,  # noqa: PLC0415
        )
        from ..models.gateway_settings_tool_ignores import (
            GatewaySettingsToolIgnores,  # noqa: PLC0415
        )
        from ..models.responses_settings import ResponsesSettings  # noqa: PLC0415

        d = dict(src_dict)
        enabled = d.pop("enabled", UNSET)

        host = d.pop("host", UNSET)

        port = d.pop("port", UNSET)

        allow_network = d.pop("allow_network", UNSET)

        require_token = d.pop("require_token", UNSET)

        cors_origins = cast(list[str], d.pop("cors_origins", UNSET))

        max_loaded_models = d.pop("max_loaded_models", UNSET)

        max_queue = d.pop("max_queue", UNSET)

        request_timeout_secs = d.pop("request_timeout_secs", UNSET)

        def _parse_default_ttl_secs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        default_ttl_secs = _parse_default_ttl_secs(d.pop("default_ttl_secs", UNSET))

        memory_aware = d.pop("memory_aware", UNSET)

        memory_headroom_mib = d.pop("memory_headroom_mib", UNSET)

        _tool_ignores = d.pop("tool_ignores", UNSET)
        tool_ignores: GatewaySettingsToolIgnores | Unset
        if isinstance(_tool_ignores, Unset):
            tool_ignores = UNSET
        else:
            tool_ignores = GatewaySettingsToolIgnores.from_dict(_tool_ignores)

        _server_tools = d.pop("server_tools", UNSET)
        server_tools: GatewaySettingsServerTools | Unset
        if isinstance(_server_tools, Unset):
            server_tools = UNSET
        else:
            server_tools = GatewaySettingsServerTools.from_dict(_server_tools)

        _text_tool_calls = d.pop("text_tool_calls", UNSET)
        text_tool_calls: GatewaySettingsTextToolCalls | Unset
        if isinstance(_text_tool_calls, Unset):
            text_tool_calls = UNSET
        else:
            text_tool_calls = GatewaySettingsTextToolCalls.from_dict(_text_tool_calls)

        ollama_port_enabled = d.pop("ollama_port_enabled", UNSET)

        ollama_port = d.pop("ollama_port", UNSET)

        ollama_allow_pull = d.pop("ollama_allow_pull", UNSET)

        _responses = d.pop("responses", UNSET)
        responses: ResponsesSettings | Unset
        if isinstance(_responses, Unset):
            responses = UNSET
        else:
            responses = ResponsesSettings.from_dict(_responses)

        gateway_settings = cls(
            enabled=enabled,
            host=host,
            port=port,
            allow_network=allow_network,
            require_token=require_token,
            cors_origins=cors_origins,
            max_loaded_models=max_loaded_models,
            max_queue=max_queue,
            request_timeout_secs=request_timeout_secs,
            default_ttl_secs=default_ttl_secs,
            memory_aware=memory_aware,
            memory_headroom_mib=memory_headroom_mib,
            tool_ignores=tool_ignores,
            server_tools=server_tools,
            text_tool_calls=text_tool_calls,
            ollama_port_enabled=ollama_port_enabled,
            ollama_port=ollama_port,
            ollama_allow_pull=ollama_allow_pull,
            responses=responses,
        )

        gateway_settings.additional_properties = d
        return gateway_settings

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
