from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ServerTotals")


@_attrs_define
class ServerTotals:
    """Server-side counters since start (`/metrics`).

    Attributes:
        prompt_tokens (int):
        generated_tokens (int):
        prompt_seconds (float):
        generation_seconds (float):
        requests_observed (int): Requests the poller saw complete (counters only move when a request finishes,
            so requests finishing within the same second count once).
        cached_prompt_tokens (int | None | Unset):
        max_sequence_tokens (int | None | Unset): Largest sequence length (prompt + generation) seen so far.
    """

    prompt_tokens: int
    generated_tokens: int
    prompt_seconds: float
    generation_seconds: float
    requests_observed: int
    cached_prompt_tokens: int | None | Unset = UNSET
    max_sequence_tokens: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        prompt_tokens = self.prompt_tokens

        generated_tokens = self.generated_tokens

        prompt_seconds = self.prompt_seconds

        generation_seconds = self.generation_seconds

        requests_observed = self.requests_observed

        cached_prompt_tokens: int | None | Unset
        if isinstance(self.cached_prompt_tokens, Unset):
            cached_prompt_tokens = UNSET
        else:
            cached_prompt_tokens = self.cached_prompt_tokens

        max_sequence_tokens: int | None | Unset
        if isinstance(self.max_sequence_tokens, Unset):
            max_sequence_tokens = UNSET
        else:
            max_sequence_tokens = self.max_sequence_tokens

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "prompt_tokens": prompt_tokens,
                "generated_tokens": generated_tokens,
                "prompt_seconds": prompt_seconds,
                "generation_seconds": generation_seconds,
                "requests_observed": requests_observed,
            }
        )
        if cached_prompt_tokens is not UNSET:
            field_dict["cached_prompt_tokens"] = cached_prompt_tokens
        if max_sequence_tokens is not UNSET:
            field_dict["max_sequence_tokens"] = max_sequence_tokens

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        prompt_tokens = d.pop("prompt_tokens")

        generated_tokens = d.pop("generated_tokens")

        prompt_seconds = d.pop("prompt_seconds")

        generation_seconds = d.pop("generation_seconds")

        requests_observed = d.pop("requests_observed")

        def _parse_cached_prompt_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        cached_prompt_tokens = _parse_cached_prompt_tokens(d.pop("cached_prompt_tokens", UNSET))

        def _parse_max_sequence_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        max_sequence_tokens = _parse_max_sequence_tokens(d.pop("max_sequence_tokens", UNSET))

        server_totals = cls(
            prompt_tokens=prompt_tokens,
            generated_tokens=generated_tokens,
            prompt_seconds=prompt_seconds,
            generation_seconds=generation_seconds,
            requests_observed=requests_observed,
            cached_prompt_tokens=cached_prompt_tokens,
            max_sequence_tokens=max_sequence_tokens,
        )

        server_totals.additional_properties = d
        return server_totals

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
