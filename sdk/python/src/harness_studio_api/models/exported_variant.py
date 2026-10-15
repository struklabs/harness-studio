from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ExportedVariant")


@_attrs_define
class ExportedVariant:
    """
    Attributes:
        id (str | Unset):  Default: ''.
        content (str | Unset):  Default: ''.
        reasoning (None | str | Unset):
        tool_calls (Any | Unset):
        stats (Any | Unset):
        finish_reason (None | str | Unset):
        stopped (bool | Unset):  Default: False.
        tool_limit_reached (bool | Unset):  Default: False.
        error (None | str | Unset):
        model_alias (None | str | Unset):
        created_at (int | Unset):  Default: 0.
        meta (Any | Unset): Client metadata (split-view side...). Absent in files written before schema v4.
    """

    id: str | Unset = ""
    content: str | Unset = ""
    reasoning: None | str | Unset = UNSET
    tool_calls: Any | Unset = UNSET
    stats: Any | Unset = UNSET
    finish_reason: None | str | Unset = UNSET
    stopped: bool | Unset = False
    tool_limit_reached: bool | Unset = False
    error: None | str | Unset = UNSET
    model_alias: None | str | Unset = UNSET
    created_at: int | Unset = 0
    meta: Any | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        content = self.content

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

        stopped = self.stopped

        tool_limit_reached = self.tool_limit_reached

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

        created_at = self.created_at

        meta = self.meta

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if id is not UNSET:
            field_dict["id"] = id
        if content is not UNSET:
            field_dict["content"] = content
        if reasoning is not UNSET:
            field_dict["reasoning"] = reasoning
        if tool_calls is not UNSET:
            field_dict["tool_calls"] = tool_calls
        if stats is not UNSET:
            field_dict["stats"] = stats
        if finish_reason is not UNSET:
            field_dict["finish_reason"] = finish_reason
        if stopped is not UNSET:
            field_dict["stopped"] = stopped
        if tool_limit_reached is not UNSET:
            field_dict["tool_limit_reached"] = tool_limit_reached
        if error is not UNSET:
            field_dict["error"] = error
        if model_alias is not UNSET:
            field_dict["model_alias"] = model_alias
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if meta is not UNSET:
            field_dict["meta"] = meta

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id", UNSET)

        content = d.pop("content", UNSET)

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

        stopped = d.pop("stopped", UNSET)

        tool_limit_reached = d.pop("tool_limit_reached", UNSET)

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

        created_at = d.pop("created_at", UNSET)

        meta = d.pop("meta", UNSET)

        exported_variant = cls(
            id=id,
            content=content,
            reasoning=reasoning,
            tool_calls=tool_calls,
            stats=stats,
            finish_reason=finish_reason,
            stopped=stopped,
            tool_limit_reached=tool_limit_reached,
            error=error,
            model_alias=model_alias,
            created_at=created_at,
            meta=meta,
        )

        exported_variant.additional_properties = d
        return exported_variant

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
