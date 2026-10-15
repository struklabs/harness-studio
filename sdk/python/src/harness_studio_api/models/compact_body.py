from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="CompactBody")


@_attrs_define
class CompactBody:
    """
    Attributes:
        model (None | str | Unset): Gateway alias that writes the summary and whose context window decides the plan
            (default: the conversation's own model).
        n_ctx (int | None | Unset): Context window of the chat model in tokens (default: the model's loaded server, else
            the definition's `context_size`).
        keep_turns (int | None | Unset): Newest turns kept as they are (default 2; fewer when they exceed 40 % of the
            context).
        clear_tool_results (bool | None | Unset): Shorten old tool results before summarizing (default true).
        summary_tokens (int | None | Unset): Summary budget in tokens (default 1/8 of the context, 256 to 2048).
        instruction (None | str | Unset): Extra instruction for the summary ("keep the numbers").
    """

    model: None | str | Unset = UNSET
    n_ctx: int | None | Unset = UNSET
    keep_turns: int | None | Unset = UNSET
    clear_tool_results: bool | None | Unset = UNSET
    summary_tokens: int | None | Unset = UNSET
    instruction: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        n_ctx: int | None | Unset
        if isinstance(self.n_ctx, Unset):
            n_ctx = UNSET
        else:
            n_ctx = self.n_ctx

        keep_turns: int | None | Unset
        if isinstance(self.keep_turns, Unset):
            keep_turns = UNSET
        else:
            keep_turns = self.keep_turns

        clear_tool_results: bool | None | Unset
        if isinstance(self.clear_tool_results, Unset):
            clear_tool_results = UNSET
        else:
            clear_tool_results = self.clear_tool_results

        summary_tokens: int | None | Unset
        if isinstance(self.summary_tokens, Unset):
            summary_tokens = UNSET
        else:
            summary_tokens = self.summary_tokens

        instruction: None | str | Unset
        if isinstance(self.instruction, Unset):
            instruction = UNSET
        else:
            instruction = self.instruction

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if model is not UNSET:
            field_dict["model"] = model
        if n_ctx is not UNSET:
            field_dict["n_ctx"] = n_ctx
        if keep_turns is not UNSET:
            field_dict["keep_turns"] = keep_turns
        if clear_tool_results is not UNSET:
            field_dict["clear_tool_results"] = clear_tool_results
        if summary_tokens is not UNSET:
            field_dict["summary_tokens"] = summary_tokens
        if instruction is not UNSET:
            field_dict["instruction"] = instruction

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_n_ctx(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        n_ctx = _parse_n_ctx(d.pop("n_ctx", UNSET))

        def _parse_keep_turns(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        keep_turns = _parse_keep_turns(d.pop("keep_turns", UNSET))

        def _parse_clear_tool_results(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        clear_tool_results = _parse_clear_tool_results(d.pop("clear_tool_results", UNSET))

        def _parse_summary_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        summary_tokens = _parse_summary_tokens(d.pop("summary_tokens", UNSET))

        def _parse_instruction(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        instruction = _parse_instruction(d.pop("instruction", UNSET))

        compact_body = cls(
            model=model,
            n_ctx=n_ctx,
            keep_turns=keep_turns,
            clear_tool_results=clear_tool_results,
            summary_tokens=summary_tokens,
            instruction=instruction,
        )

        compact_body.additional_properties = d
        return compact_body

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
