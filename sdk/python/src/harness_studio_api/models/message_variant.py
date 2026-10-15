from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="MessageVariant")


@_attrs_define
class MessageVariant:
    """
    Attributes:
        id (str):
        message_id (str):
        content (str):
        stopped (bool):
        tool_limit_reached (bool):
        created_at (int):
        reasoning (None | str | Unset):
        tool_calls (Any | Unset):
        stats (Any | Unset):
        finish_reason (None | str | Unset):
        error (None | str | Unset):
        model_alias (None | str | Unset):
        meta (Any | Unset):
    """

    id: str
    message_id: str
    content: str
    stopped: bool
    tool_limit_reached: bool
    created_at: int
    reasoning: None | str | Unset = UNSET
    tool_calls: Any | Unset = UNSET
    stats: Any | Unset = UNSET
    finish_reason: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    model_alias: None | str | Unset = UNSET
    meta: Any | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        message_id = self.message_id

        content = self.content

        stopped = self.stopped

        tool_limit_reached = self.tool_limit_reached

        created_at = self.created_at

        reasoning: None | str | Unset
        if isinstance(self.reasoning, Unset):
            reasoning = UNSET
        else:
            reasoning = self.reasoning

        tool_calls = self.tool_calls

        stats = self.stats

        finish_reason: None | str | Unset
        if isinstance(self.finish_reason, Unset):
            finish_reason = UNSET
        else:
            finish_reason = self.finish_reason

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        model_alias: None | str | Unset
        if isinstance(self.model_alias, Unset):
            model_alias = UNSET
        else:
            model_alias = self.model_alias

        meta = self.meta

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "message_id": message_id,
                "content": content,
                "stopped": stopped,
                "tool_limit_reached": tool_limit_reached,
                "created_at": created_at,
            }
        )
        if reasoning is not UNSET:
            field_dict["reasoning"] = reasoning
        if tool_calls is not UNSET:
            field_dict["tool_calls"] = tool_calls
        if stats is not UNSET:
            field_dict["stats"] = stats
        if finish_reason is not UNSET:
            field_dict["finish_reason"] = finish_reason
        if error is not UNSET:
            field_dict["error"] = error
        if model_alias is not UNSET:
            field_dict["model_alias"] = model_alias
        if meta is not UNSET:
            field_dict["meta"] = meta

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        message_id = d.pop("message_id")

        content = d.pop("content")

        stopped = d.pop("stopped")

        tool_limit_reached = d.pop("tool_limit_reached")

        created_at = d.pop("created_at")

        def _parse_reasoning(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reasoning = _parse_reasoning(d.pop("reasoning", UNSET))

        tool_calls = d.pop("tool_calls", UNSET)

        stats = d.pop("stats", UNSET)

        def _parse_finish_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        finish_reason = _parse_finish_reason(d.pop("finish_reason", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_model_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model_alias = _parse_model_alias(d.pop("model_alias", UNSET))

        meta = d.pop("meta", UNSET)

        message_variant = cls(
            id=id,
            message_id=message_id,
            content=content,
            stopped=stopped,
            tool_limit_reached=tool_limit_reached,
            created_at=created_at,
            reasoning=reasoning,
            tool_calls=tool_calls,
            stats=stats,
            finish_reason=finish_reason,
            error=error,
            model_alias=model_alias,
            meta=meta,
        )

        message_variant.additional_properties = d
        return message_variant

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
