from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RateLimits")


@_attrs_define
class RateLimits:
    """Rate limits of one token (`None` = unlimited).

    Attributes:
        requests_per_minute (int | None | Unset): Requests admitted per minute (token bucket: bursts up to this many,
            refilled evenly).
        tokens_per_minute (int | None | Unset): Prompt + completion tokens per minute. A request is admitted while the
            budget is
            positive; its tokens are charged when it ends (so one long answer can overdraw it,
            and the next request waits until the budget recovers).
        max_concurrent (int | None | Unset): Requests of this token in flight at once (streams count until they end).
    """

    requests_per_minute: int | None | Unset = UNSET
    tokens_per_minute: int | None | Unset = UNSET
    max_concurrent: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        requests_per_minute: int | None | Unset
        if isinstance(self.requests_per_minute, Unset):
            requests_per_minute = UNSET
        else:
            requests_per_minute = self.requests_per_minute

        tokens_per_minute: int | None | Unset
        if isinstance(self.tokens_per_minute, Unset):
            tokens_per_minute = UNSET
        else:
            tokens_per_minute = self.tokens_per_minute

        max_concurrent: int | None | Unset
        if isinstance(self.max_concurrent, Unset):
            max_concurrent = UNSET
        else:
            max_concurrent = self.max_concurrent

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if requests_per_minute is not UNSET:
            field_dict["requests_per_minute"] = requests_per_minute
        if tokens_per_minute is not UNSET:
            field_dict["tokens_per_minute"] = tokens_per_minute
        if max_concurrent is not UNSET:
            field_dict["max_concurrent"] = max_concurrent

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_requests_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requests_per_minute = _parse_requests_per_minute(d.pop("requests_per_minute", UNSET))

        def _parse_tokens_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        tokens_per_minute = _parse_tokens_per_minute(d.pop("tokens_per_minute", UNSET))

        def _parse_max_concurrent(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        max_concurrent = _parse_max_concurrent(d.pop("max_concurrent", UNSET))

        rate_limits = cls(
            requests_per_minute=requests_per_minute,
            tokens_per_minute=tokens_per_minute,
            max_concurrent=max_concurrent,
        )

        rate_limits.additional_properties = d
        return rate_limits

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
