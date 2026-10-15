from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ToolCallRow")


@_attrs_define
class ToolCallRow:
    """One tool call as stored.

    Attributes:
        id (str):
        started_at_ms (int):
        client (str): The client: `playground`, `cli`, `api`...
        tool (str):
        class_ (str): `safe`, `restricted`, `dangerous`.
        source (str): `builtin`, `mcp`, `installed`, `generated`.
        args (str): The arguments as JSON text, long strings shortened.
        approved (bool): The user approved this call when the policy asked.
        duration_ms (int):
        result_chars (int):
        truncated (bool):
        conversation (None | str | Unset):
        model (None | str | Unset):
        error_code (None | str | Unset): `ToolErrorCode` name; `None` = the call succeeded.
        network_target (None | str | Unset): Host a network tool contacted.
    """

    id: str
    started_at_ms: int
    client: str
    tool: str
    class_: str
    source: str
    args: str
    approved: bool
    duration_ms: int
    result_chars: int
    truncated: bool
    conversation: None | str | Unset = UNSET
    model: None | str | Unset = UNSET
    error_code: None | str | Unset = UNSET
    network_target: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        started_at_ms = self.started_at_ms

        client = self.client

        tool = self.tool

        class_ = self.class_

        source = self.source

        args = self.args

        approved = self.approved

        duration_ms = self.duration_ms

        result_chars = self.result_chars

        truncated = self.truncated

        conversation: None | str | Unset
        if isinstance(self.conversation, Unset):
            conversation = UNSET
        else:
            conversation = self.conversation

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        error_code: None | str | Unset
        if isinstance(self.error_code, Unset):
            error_code = UNSET
        else:
            error_code = self.error_code

        network_target: None | str | Unset
        if isinstance(self.network_target, Unset):
            network_target = UNSET
        else:
            network_target = self.network_target

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "started_at_ms": started_at_ms,
                "client": client,
                "tool": tool,
                "class": class_,
                "source": source,
                "args": args,
                "approved": approved,
                "duration_ms": duration_ms,
                "result_chars": result_chars,
                "truncated": truncated,
            }
        )
        if conversation is not UNSET:
            field_dict["conversation"] = conversation
        if model is not UNSET:
            field_dict["model"] = model
        if error_code is not UNSET:
            field_dict["error_code"] = error_code
        if network_target is not UNSET:
            field_dict["network_target"] = network_target

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        started_at_ms = d.pop("started_at_ms")

        client = d.pop("client")

        tool = d.pop("tool")

        class_ = d.pop("class")

        source = d.pop("source")

        args = d.pop("args")

        approved = d.pop("approved")

        duration_ms = d.pop("duration_ms")

        result_chars = d.pop("result_chars")

        truncated = d.pop("truncated")

        def _parse_conversation(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        conversation = _parse_conversation(d.pop("conversation", UNSET))

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_error_code(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error_code = _parse_error_code(d.pop("error_code", UNSET))

        def _parse_network_target(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        network_target = _parse_network_target(d.pop("network_target", UNSET))

        tool_call_row = cls(
            id=id,
            started_at_ms=started_at_ms,
            client=client,
            tool=tool,
            class_=class_,
            source=source,
            args=args,
            approved=approved,
            duration_ms=duration_ms,
            result_chars=result_chars,
            truncated=truncated,
            conversation=conversation,
            model=model,
            error_code=error_code,
            network_target=network_target,
        )

        tool_call_row.additional_properties = d
        return tool_call_row

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
