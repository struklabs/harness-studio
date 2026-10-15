from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.client_info import ClientInfo
    from ..models.usage import Usage


T = TypeVar("T", bound="RequestRecord")


@_attrs_define
class RequestRecord:
    """Metadata of one gateway request.

    Attributes:
        id (str): Gateway request id (`X-Harness-Request-Id`).
        started_at_ms (int): Unix milliseconds of arrival.
        endpoint (str): Route, e.g. `/v1/chat/completions`.
        client (ClientInfo): Who sent a request.
        stream (bool):
        status (int): HTTP status returned to the client.
        cancelled (bool): The client went away before the response ended (the upstream request was cancelled).
        queue_wait_ms (int): Waiting for a free slot (and for a JIT load), ms.
        evicted (list[str]): Models evicted to make room for this request's load.
        total_ms (int):
        usage (Usage): Token counts and llama-server timings read from a response.
        compat (list[str]): Compatibility changes (`namespace_flattened=3`, `ignored=web_search`).
        model (None | str | Unset): `model` as sent.
        alias (None | str | Unset):
        definition_id (None | str | Unset):
        server_id (None | str | Unset):
        error_code (None | str | Unset): Gateway error code (`queue_full`, `unsupported_tool_type`,
            `insufficient_memory`...).
        load_ms (int | None | Unset): JIT load time when this request loaded the model, ms.
        ttft_ms (int | None | Unset): First response byte, ms from arrival.
        oom (bool | Unset): Ran out of memory: no room to load, the load hit OOM, or it did and the retry after
            an eviction worked.
        ctx_size (int | None | Unset): Context window of one slot of the server that answered (`/props`).
        backend (None | str | Unset): ggml backend of that server's build (`vulkan`, `cuda`...).
        slots (int | None | Unset): Slots (concurrent requests) of the server that answered.
        waited (None | str | Unset): Why the request waited for its turn: `slots` (all busy) or `tokens` (the shared KV
            pool
            had no room yet); `None` = it ran at once. Feeds the slot recommendation.
        tool_involved (bool | Unset): The request or its response involves tools (offered, called or reported on).
        tools_offered (int | Unset): Tools the request offered.
        tool_calls (int | Unset): Tool calls the response made.
        tool_diag_category (None | str | Unset): Who is at fault when the round went wrong: `runtime`, `context`,
            `model` or `client`;
            `None` for a clean round.
        tool_diag_code (None | str | Unset): What went wrong exactly (`call_left_in_content`, `repeated_call`...), see
            `tool_diag::Code`.
        tool_diag_detail (None | str | Unset): Evidence as JSON text (tool name, excerpt, schema path, linked
            request...).
    """

    id: str
    started_at_ms: int
    endpoint: str
    client: ClientInfo
    stream: bool
    status: int
    cancelled: bool
    queue_wait_ms: int
    evicted: list[str]
    total_ms: int
    usage: Usage
    compat: list[str]
    model: None | str | Unset = UNSET
    alias: None | str | Unset = UNSET
    definition_id: None | str | Unset = UNSET
    server_id: None | str | Unset = UNSET
    error_code: None | str | Unset = UNSET
    load_ms: int | None | Unset = UNSET
    ttft_ms: int | None | Unset = UNSET
    oom: bool | Unset = UNSET
    ctx_size: int | None | Unset = UNSET
    backend: None | str | Unset = UNSET
    slots: int | None | Unset = UNSET
    waited: None | str | Unset = UNSET
    tool_involved: bool | Unset = UNSET
    tools_offered: int | Unset = UNSET
    tool_calls: int | Unset = UNSET
    tool_diag_category: None | str | Unset = UNSET
    tool_diag_code: None | str | Unset = UNSET
    tool_diag_detail: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        started_at_ms = self.started_at_ms

        endpoint = self.endpoint

        client = self.client.to_dict()

        stream = self.stream

        status = self.status

        cancelled = self.cancelled

        queue_wait_ms = self.queue_wait_ms

        evicted = self.evicted

        total_ms = self.total_ms

        usage = self.usage.to_dict()

        compat = self.compat

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        alias: None | str | Unset
        if isinstance(self.alias, Unset):
            alias = UNSET
        else:
            alias = self.alias

        definition_id: None | str | Unset
        if isinstance(self.definition_id, Unset):
            definition_id = UNSET
        else:
            definition_id = self.definition_id

        server_id: None | str | Unset
        if isinstance(self.server_id, Unset):
            server_id = UNSET
        else:
            server_id = self.server_id

        error_code: None | str | Unset
        if isinstance(self.error_code, Unset):
            error_code = UNSET
        else:
            error_code = self.error_code

        load_ms: int | None | Unset
        if isinstance(self.load_ms, Unset):
            load_ms = UNSET
        else:
            load_ms = self.load_ms

        ttft_ms: int | None | Unset
        if isinstance(self.ttft_ms, Unset):
            ttft_ms = UNSET
        else:
            ttft_ms = self.ttft_ms

        oom = self.oom

        ctx_size: int | None | Unset
        if isinstance(self.ctx_size, Unset):
            ctx_size = UNSET
        else:
            ctx_size = self.ctx_size

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        slots: int | None | Unset
        if isinstance(self.slots, Unset):
            slots = UNSET
        else:
            slots = self.slots

        waited: None | str | Unset
        if isinstance(self.waited, Unset):
            waited = UNSET
        else:
            waited = self.waited

        tool_involved = self.tool_involved

        tools_offered = self.tools_offered

        tool_calls = self.tool_calls

        tool_diag_category: None | str | Unset
        if isinstance(self.tool_diag_category, Unset):
            tool_diag_category = UNSET
        else:
            tool_diag_category = self.tool_diag_category

        tool_diag_code: None | str | Unset
        if isinstance(self.tool_diag_code, Unset):
            tool_diag_code = UNSET
        else:
            tool_diag_code = self.tool_diag_code

        tool_diag_detail: None | str | Unset
        if isinstance(self.tool_diag_detail, Unset):
            tool_diag_detail = UNSET
        else:
            tool_diag_detail = self.tool_diag_detail

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "started_at_ms": started_at_ms,
                "endpoint": endpoint,
                "client": client,
                "stream": stream,
                "status": status,
                "cancelled": cancelled,
                "queue_wait_ms": queue_wait_ms,
                "evicted": evicted,
                "total_ms": total_ms,
                "usage": usage,
                "compat": compat,
            }
        )
        if model is not UNSET:
            field_dict["model"] = model
        if alias is not UNSET:
            field_dict["alias"] = alias
        if definition_id is not UNSET:
            field_dict["definition_id"] = definition_id
        if server_id is not UNSET:
            field_dict["server_id"] = server_id
        if error_code is not UNSET:
            field_dict["error_code"] = error_code
        if load_ms is not UNSET:
            field_dict["load_ms"] = load_ms
        if ttft_ms is not UNSET:
            field_dict["ttft_ms"] = ttft_ms
        if oom is not UNSET:
            field_dict["oom"] = oom
        if ctx_size is not UNSET:
            field_dict["ctx_size"] = ctx_size
        if backend is not UNSET:
            field_dict["backend"] = backend
        if slots is not UNSET:
            field_dict["slots"] = slots
        if waited is not UNSET:
            field_dict["waited"] = waited
        if tool_involved is not UNSET:
            field_dict["tool_involved"] = tool_involved
        if tools_offered is not UNSET:
            field_dict["tools_offered"] = tools_offered
        if tool_calls is not UNSET:
            field_dict["tool_calls"] = tool_calls
        if tool_diag_category is not UNSET:
            field_dict["tool_diag_category"] = tool_diag_category
        if tool_diag_code is not UNSET:
            field_dict["tool_diag_code"] = tool_diag_code
        if tool_diag_detail is not UNSET:
            field_dict["tool_diag_detail"] = tool_diag_detail

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.client_info import ClientInfo  # noqa: PLC0415
        from ..models.usage import Usage  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        started_at_ms = d.pop("started_at_ms")

        endpoint = d.pop("endpoint")

        client = ClientInfo.from_dict(d.pop("client"))

        stream = d.pop("stream")

        status = d.pop("status")

        cancelled = d.pop("cancelled")

        queue_wait_ms = d.pop("queue_wait_ms")

        evicted = cast(list[str], d.pop("evicted"))

        total_ms = d.pop("total_ms")

        usage = Usage.from_dict(d.pop("usage"))

        compat = cast(list[str], d.pop("compat"))

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        alias = _parse_alias(d.pop("alias", UNSET))

        def _parse_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        definition_id = _parse_definition_id(d.pop("definition_id", UNSET))

        def _parse_server_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        server_id = _parse_server_id(d.pop("server_id", UNSET))

        def _parse_error_code(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error_code = _parse_error_code(d.pop("error_code", UNSET))

        def _parse_load_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        load_ms = _parse_load_ms(d.pop("load_ms", UNSET))

        def _parse_ttft_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ttft_ms = _parse_ttft_ms(d.pop("ttft_ms", UNSET))

        oom = d.pop("oom", UNSET)

        def _parse_ctx_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ctx_size = _parse_ctx_size(d.pop("ctx_size", UNSET))

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        def _parse_slots(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        slots = _parse_slots(d.pop("slots", UNSET))

        def _parse_waited(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        waited = _parse_waited(d.pop("waited", UNSET))

        tool_involved = d.pop("tool_involved", UNSET)

        tools_offered = d.pop("tools_offered", UNSET)

        tool_calls = d.pop("tool_calls", UNSET)

        def _parse_tool_diag_category(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tool_diag_category = _parse_tool_diag_category(d.pop("tool_diag_category", UNSET))

        def _parse_tool_diag_code(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tool_diag_code = _parse_tool_diag_code(d.pop("tool_diag_code", UNSET))

        def _parse_tool_diag_detail(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tool_diag_detail = _parse_tool_diag_detail(d.pop("tool_diag_detail", UNSET))

        request_record = cls(
            id=id,
            started_at_ms=started_at_ms,
            endpoint=endpoint,
            client=client,
            stream=stream,
            status=status,
            cancelled=cancelled,
            queue_wait_ms=queue_wait_ms,
            evicted=evicted,
            total_ms=total_ms,
            usage=usage,
            compat=compat,
            model=model,
            alias=alias,
            definition_id=definition_id,
            server_id=server_id,
            error_code=error_code,
            load_ms=load_ms,
            ttft_ms=ttft_ms,
            oom=oom,
            ctx_size=ctx_size,
            backend=backend,
            slots=slots,
            waited=waited,
            tool_involved=tool_involved,
            tools_offered=tools_offered,
            tool_calls=tool_calls,
            tool_diag_category=tool_diag_category,
            tool_diag_code=tool_diag_code,
            tool_diag_detail=tool_diag_detail,
        )

        request_record.additional_properties = d
        return request_record

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
