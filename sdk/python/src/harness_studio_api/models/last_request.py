from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="LastRequest")


@_attrs_define
class LastRequest:
    """Work done by the request(s) that finished between two samples.

    Attributes:
        finished_at (int):
        prompt_tokens (int):
        cached_tokens (int):
        generated_tokens (int):
        prompt_seconds (float): Time spent on the prompt: the closest thing to time-to-first-token /metrics offers.
        generation_seconds (float):
    """

    finished_at: int
    prompt_tokens: int
    cached_tokens: int
    generated_tokens: int
    prompt_seconds: float
    generation_seconds: float
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        finished_at = self.finished_at

        prompt_tokens = self.prompt_tokens

        cached_tokens = self.cached_tokens

        generated_tokens = self.generated_tokens

        prompt_seconds = self.prompt_seconds

        generation_seconds = self.generation_seconds

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "finished_at": finished_at,
                "prompt_tokens": prompt_tokens,
                "cached_tokens": cached_tokens,
                "generated_tokens": generated_tokens,
                "prompt_seconds": prompt_seconds,
                "generation_seconds": generation_seconds,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        finished_at = d.pop("finished_at")

        prompt_tokens = d.pop("prompt_tokens")

        cached_tokens = d.pop("cached_tokens")

        generated_tokens = d.pop("generated_tokens")

        prompt_seconds = d.pop("prompt_seconds")

        generation_seconds = d.pop("generation_seconds")

        last_request = cls(
            finished_at=finished_at,
            prompt_tokens=prompt_tokens,
            cached_tokens=cached_tokens,
            generated_tokens=generated_tokens,
            prompt_seconds=prompt_seconds,
            generation_seconds=generation_seconds,
        )

        last_request.additional_properties = d
        return last_request

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
