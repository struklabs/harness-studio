from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RequestRow")


@_attrs_define
class RequestRow:
    """One stored gateway request (metadata only).

    Attributes:
        id (str):
        started_at_ms (int):
        endpoint (str):
        stream (bool):
        status (int):
        oom (bool):
        cancelled (bool):
        queue_wait_ms (int):
        evicted (list[str]):
        total_ms (int):
        compat (list[str]):
        content_captured (bool): The content of this request was captured (see `request_content`).
        tool_involved (bool): The request offered tools, carried tool results or the response made a call.
        tools_offered (int): Tools the request offered.
        tool_calls (int): Tool calls the response made.
        model (None | str | Unset):
        alias (None | str | Unset):
        definition_id (None | str | Unset):
        server_id (None | str | Unset):
        client (None | str | Unset): Client label (`X-Harness-Client`, token name, `User-Agent` product; in that order).
        user_agent (None | str | Unset):
        harness_client (None | str | Unset):
        token_name (None | str | Unset):
        error_code (None | str | Unset):
        load_ms (int | None | Unset):
        ttft_ms (int | None | Unset):
        prompt_tokens (int | None | Unset):
        completion_tokens (int | None | Unset):
        cached_tokens (int | None | Unset):
        prompt_ms (float | None | Unset):
        predicted_ms (float | None | Unset):
        prompt_per_second (float | None | Unset):
        predicted_per_second (float | None | Unset):
        draft_n (int | None | Unset):
        draft_accepted (int | None | Unset):
        finish_reason (None | str | Unset):
        ctx_size (int | None | Unset): Context window of one slot of the server.
        context_fill (float | None | Unset): (prompt + completion tokens) / `ctx_size`.
        vram_peak_bytes (int | None | Unset): Highest used GPU memory (whole GPU) sampled during the request (1 Hz
            telemetry).
        ram_peak_bytes (int | None | Unset): Highest used system RAM sampled during the request.
        backend (None | str | Unset):
        slots (int | None | Unset): Slots of the server that answered.
        waited (None | str | Unset): `slots` / `tokens` when the request waited for its turn (see
            `RequestRecord::waited`).
        tool_diag_category (None | str | Unset): Who is at fault: `runtime`, `context`, `model` or `client`; `None` for
            a clean round.
        tool_diag_code (None | str | Unset): The exact finding (`call_left_in_content`, `repeated_call`...).
        tool_diag_detail (None | str | Unset): Evidence as JSON text (tool name, excerpt, schema path, linked
            request...).
    """

    id: str
    started_at_ms: int
    endpoint: str
    stream: bool
    status: int
    oom: bool
    cancelled: bool
    queue_wait_ms: int
    evicted: list[str]
    total_ms: int
    compat: list[str]
    content_captured: bool
    tool_involved: bool
    tools_offered: int
    tool_calls: int
    model: None | str | Unset = UNSET
    alias: None | str | Unset = UNSET
    definition_id: None | str | Unset = UNSET
    server_id: None | str | Unset = UNSET
    client: None | str | Unset = UNSET
    user_agent: None | str | Unset = UNSET
    harness_client: None | str | Unset = UNSET
    token_name: None | str | Unset = UNSET
    error_code: None | str | Unset = UNSET
    load_ms: int | None | Unset = UNSET
    ttft_ms: int | None | Unset = UNSET
    prompt_tokens: int | None | Unset = UNSET
    completion_tokens: int | None | Unset = UNSET
    cached_tokens: int | None | Unset = UNSET
    prompt_ms: float | None | Unset = UNSET
    predicted_ms: float | None | Unset = UNSET
    prompt_per_second: float | None | Unset = UNSET
    predicted_per_second: float | None | Unset = UNSET
    draft_n: int | None | Unset = UNSET
    draft_accepted: int | None | Unset = UNSET
    finish_reason: None | str | Unset = UNSET
    ctx_size: int | None | Unset = UNSET
    context_fill: float | None | Unset = UNSET
    vram_peak_bytes: int | None | Unset = UNSET
    ram_peak_bytes: int | None | Unset = UNSET
    backend: None | str | Unset = UNSET
    slots: int | None | Unset = UNSET
    waited: None | str | Unset = UNSET
    tool_diag_category: None | str | Unset = UNSET
    tool_diag_code: None | str | Unset = UNSET
    tool_diag_detail: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        started_at_ms = self.started_at_ms

        endpoint = self.endpoint

        stream = self.stream

        status = self.status

        oom = self.oom

        cancelled = self.cancelled

        queue_wait_ms = self.queue_wait_ms

        evicted = self.evicted

        total_ms = self.total_ms

        compat = self.compat

        content_captured = self.content_captured

        tool_involved = self.tool_involved

        tools_offered = self.tools_offered

        tool_calls = self.tool_calls

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

        client: None | str | Unset
        if isinstance(self.client, Unset):
            client = UNSET
        else:
            client = self.client

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

        prompt_tokens: int | None | Unset
        if isinstance(self.prompt_tokens, Unset):
            prompt_tokens = UNSET
        else:
            prompt_tokens = self.prompt_tokens

        completion_tokens: int | None | Unset
        if isinstance(self.completion_tokens, Unset):
            completion_tokens = UNSET
        else:
            completion_tokens = self.completion_tokens

        cached_tokens: int | None | Unset
        if isinstance(self.cached_tokens, Unset):
            cached_tokens = UNSET
        else:
            cached_tokens = self.cached_tokens

        prompt_ms: float | None | Unset
        if isinstance(self.prompt_ms, Unset):
            prompt_ms = UNSET
        else:
            prompt_ms = self.prompt_ms

        predicted_ms: float | None | Unset
        if isinstance(self.predicted_ms, Unset):
            predicted_ms = UNSET
        else:
            predicted_ms = self.predicted_ms

        prompt_per_second: float | None | Unset
        if isinstance(self.prompt_per_second, Unset):
            prompt_per_second = UNSET
        else:
            prompt_per_second = self.prompt_per_second

        predicted_per_second: float | None | Unset
        if isinstance(self.predicted_per_second, Unset):
            predicted_per_second = UNSET
        else:
            predicted_per_second = self.predicted_per_second

        draft_n: int | None | Unset
        if isinstance(self.draft_n, Unset):
            draft_n = UNSET
        else:
            draft_n = self.draft_n

        draft_accepted: int | None | Unset
        if isinstance(self.draft_accepted, Unset):
            draft_accepted = UNSET
        else:
            draft_accepted = self.draft_accepted

        finish_reason: None | str | Unset
        if isinstance(self.finish_reason, Unset):
            finish_reason = UNSET
        else:
            finish_reason = self.finish_reason

        ctx_size: int | None | Unset
        if isinstance(self.ctx_size, Unset):
            ctx_size = UNSET
        else:
            ctx_size = self.ctx_size

        context_fill: float | None | Unset
        if isinstance(self.context_fill, Unset):
            context_fill = UNSET
        else:
            context_fill = self.context_fill

        vram_peak_bytes: int | None | Unset
        if isinstance(self.vram_peak_bytes, Unset):
            vram_peak_bytes = UNSET
        else:
            vram_peak_bytes = self.vram_peak_bytes

        ram_peak_bytes: int | None | Unset
        if isinstance(self.ram_peak_bytes, Unset):
            ram_peak_bytes = UNSET
        else:
            ram_peak_bytes = self.ram_peak_bytes

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
                "stream": stream,
                "status": status,
                "oom": oom,
                "cancelled": cancelled,
                "queue_wait_ms": queue_wait_ms,
                "evicted": evicted,
                "total_ms": total_ms,
                "compat": compat,
                "content_captured": content_captured,
                "tool_involved": tool_involved,
                "tools_offered": tools_offered,
                "tool_calls": tool_calls,
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
        if client is not UNSET:
            field_dict["client"] = client
        if user_agent is not UNSET:
            field_dict["user_agent"] = user_agent
        if harness_client is not UNSET:
            field_dict["harness_client"] = harness_client
        if token_name is not UNSET:
            field_dict["token_name"] = token_name
        if error_code is not UNSET:
            field_dict["error_code"] = error_code
        if load_ms is not UNSET:
            field_dict["load_ms"] = load_ms
        if ttft_ms is not UNSET:
            field_dict["ttft_ms"] = ttft_ms
        if prompt_tokens is not UNSET:
            field_dict["prompt_tokens"] = prompt_tokens
        if completion_tokens is not UNSET:
            field_dict["completion_tokens"] = completion_tokens
        if cached_tokens is not UNSET:
            field_dict["cached_tokens"] = cached_tokens
        if prompt_ms is not UNSET:
            field_dict["prompt_ms"] = prompt_ms
        if predicted_ms is not UNSET:
            field_dict["predicted_ms"] = predicted_ms
        if prompt_per_second is not UNSET:
            field_dict["prompt_per_second"] = prompt_per_second
        if predicted_per_second is not UNSET:
            field_dict["predicted_per_second"] = predicted_per_second
        if draft_n is not UNSET:
            field_dict["draft_n"] = draft_n
        if draft_accepted is not UNSET:
            field_dict["draft_accepted"] = draft_accepted
        if finish_reason is not UNSET:
            field_dict["finish_reason"] = finish_reason
        if ctx_size is not UNSET:
            field_dict["ctx_size"] = ctx_size
        if context_fill is not UNSET:
            field_dict["context_fill"] = context_fill
        if vram_peak_bytes is not UNSET:
            field_dict["vram_peak_bytes"] = vram_peak_bytes
        if ram_peak_bytes is not UNSET:
            field_dict["ram_peak_bytes"] = ram_peak_bytes
        if backend is not UNSET:
            field_dict["backend"] = backend
        if slots is not UNSET:
            field_dict["slots"] = slots
        if waited is not UNSET:
            field_dict["waited"] = waited
        if tool_diag_category is not UNSET:
            field_dict["tool_diag_category"] = tool_diag_category
        if tool_diag_code is not UNSET:
            field_dict["tool_diag_code"] = tool_diag_code
        if tool_diag_detail is not UNSET:
            field_dict["tool_diag_detail"] = tool_diag_detail

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        started_at_ms = d.pop("started_at_ms")

        endpoint = d.pop("endpoint")

        stream = d.pop("stream")

        status = d.pop("status")

        oom = d.pop("oom")

        cancelled = d.pop("cancelled")

        queue_wait_ms = d.pop("queue_wait_ms")

        evicted = cast(list[str], d.pop("evicted"))

        total_ms = d.pop("total_ms")

        compat = cast(list[str], d.pop("compat"))

        content_captured = d.pop("content_captured")

        tool_involved = d.pop("tool_involved")

        tools_offered = d.pop("tools_offered")

        tool_calls = d.pop("tool_calls")

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

        def _parse_client(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        client = _parse_client(d.pop("client", UNSET))

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

        def _parse_prompt_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        prompt_tokens = _parse_prompt_tokens(d.pop("prompt_tokens", UNSET))

        def _parse_completion_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        completion_tokens = _parse_completion_tokens(d.pop("completion_tokens", UNSET))

        def _parse_cached_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        cached_tokens = _parse_cached_tokens(d.pop("cached_tokens", UNSET))

        def _parse_prompt_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        prompt_ms = _parse_prompt_ms(d.pop("prompt_ms", UNSET))

        def _parse_predicted_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        predicted_ms = _parse_predicted_ms(d.pop("predicted_ms", UNSET))

        def _parse_prompt_per_second(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        prompt_per_second = _parse_prompt_per_second(d.pop("prompt_per_second", UNSET))

        def _parse_predicted_per_second(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        predicted_per_second = _parse_predicted_per_second(d.pop("predicted_per_second", UNSET))

        def _parse_draft_n(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        draft_n = _parse_draft_n(d.pop("draft_n", UNSET))

        def _parse_draft_accepted(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        draft_accepted = _parse_draft_accepted(d.pop("draft_accepted", UNSET))

        def _parse_finish_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        finish_reason = _parse_finish_reason(d.pop("finish_reason", UNSET))

        def _parse_ctx_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ctx_size = _parse_ctx_size(d.pop("ctx_size", UNSET))

        def _parse_context_fill(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        context_fill = _parse_context_fill(d.pop("context_fill", UNSET))

        def _parse_vram_peak_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vram_peak_bytes = _parse_vram_peak_bytes(d.pop("vram_peak_bytes", UNSET))

        def _parse_ram_peak_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ram_peak_bytes = _parse_ram_peak_bytes(d.pop("ram_peak_bytes", UNSET))

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

        request_row = cls(
            id=id,
            started_at_ms=started_at_ms,
            endpoint=endpoint,
            stream=stream,
            status=status,
            oom=oom,
            cancelled=cancelled,
            queue_wait_ms=queue_wait_ms,
            evicted=evicted,
            total_ms=total_ms,
            compat=compat,
            content_captured=content_captured,
            tool_involved=tool_involved,
            tools_offered=tools_offered,
            tool_calls=tool_calls,
            model=model,
            alias=alias,
            definition_id=definition_id,
            server_id=server_id,
            client=client,
            user_agent=user_agent,
            harness_client=harness_client,
            token_name=token_name,
            error_code=error_code,
            load_ms=load_ms,
            ttft_ms=ttft_ms,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            cached_tokens=cached_tokens,
            prompt_ms=prompt_ms,
            predicted_ms=predicted_ms,
            prompt_per_second=prompt_per_second,
            predicted_per_second=predicted_per_second,
            draft_n=draft_n,
            draft_accepted=draft_accepted,
            finish_reason=finish_reason,
            ctx_size=ctx_size,
            context_fill=context_fill,
            vram_peak_bytes=vram_peak_bytes,
            ram_peak_bytes=ram_peak_bytes,
            backend=backend,
            slots=slots,
            waited=waited,
            tool_diag_category=tool_diag_category,
            tool_diag_code=tool_diag_code,
            tool_diag_detail=tool_diag_detail,
        )

        request_row.additional_properties = d
        return request_row

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
