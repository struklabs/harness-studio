from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="Usage")


@_attrs_define
class Usage:
    """Token counts and llama-server timings read from a response.

    Attributes:
        prompt_tokens (int | None | Unset):
        completion_tokens (int | None | Unset):
        reasoning_tokens (int | None | Unset):
        cached_tokens (int | None | Unset): Prompt tokens served from the KV cache (`timings.cache_n`, `cached_tokens`).
        prompt_ms (float | None | Unset):
        predicted_ms (float | None | Unset):
        prompt_per_second (float | None | Unset):
        predicted_per_second (float | None | Unset):
        draft_n (int | None | Unset): Speculative decoding: drafted and accepted tokens.
        draft_n_accepted (int | None | Unset):
        finish_reason (None | str | Unset): `finish_reason` / `stop_reason` / Responses `status`.
    """

    prompt_tokens: int | None | Unset = UNSET
    completion_tokens: int | None | Unset = UNSET
    reasoning_tokens: int | None | Unset = UNSET
    cached_tokens: int | None | Unset = UNSET
    prompt_ms: float | None | Unset = UNSET
    predicted_ms: float | None | Unset = UNSET
    prompt_per_second: float | None | Unset = UNSET
    predicted_per_second: float | None | Unset = UNSET
    draft_n: int | None | Unset = UNSET
    draft_n_accepted: int | None | Unset = UNSET
    finish_reason: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
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

        reasoning_tokens: int | None | Unset
        if isinstance(self.reasoning_tokens, Unset):
            reasoning_tokens = UNSET
        else:
            reasoning_tokens = self.reasoning_tokens

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

        draft_n_accepted: int | None | Unset
        if isinstance(self.draft_n_accepted, Unset):
            draft_n_accepted = UNSET
        else:
            draft_n_accepted = self.draft_n_accepted

        finish_reason: None | str | Unset
        if isinstance(self.finish_reason, Unset):
            finish_reason = UNSET
        else:
            finish_reason = self.finish_reason

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if prompt_tokens is not UNSET:
            field_dict["prompt_tokens"] = prompt_tokens
        if completion_tokens is not UNSET:
            field_dict["completion_tokens"] = completion_tokens
        if reasoning_tokens is not UNSET:
            field_dict["reasoning_tokens"] = reasoning_tokens
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
        if draft_n_accepted is not UNSET:
            field_dict["draft_n_accepted"] = draft_n_accepted
        if finish_reason is not UNSET:
            field_dict["finish_reason"] = finish_reason

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

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

        def _parse_reasoning_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        reasoning_tokens = _parse_reasoning_tokens(d.pop("reasoning_tokens", UNSET))

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

        def _parse_draft_n_accepted(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        draft_n_accepted = _parse_draft_n_accepted(d.pop("draft_n_accepted", UNSET))

        def _parse_finish_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        finish_reason = _parse_finish_reason(d.pop("finish_reason", UNSET))

        usage = cls(
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            reasoning_tokens=reasoning_tokens,
            cached_tokens=cached_tokens,
            prompt_ms=prompt_ms,
            predicted_ms=predicted_ms,
            prompt_per_second=prompt_per_second,
            predicted_per_second=predicted_per_second,
            draft_n=draft_n,
            draft_n_accepted=draft_n_accepted,
            finish_reason=finish_reason,
        )

        usage.additional_properties = d
        return usage

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
