from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.kv_level import KvLevel, check_kv_level
from ..models.kv_reason import KvReason, check_kv_reason
from ..types import UNSET, Unset

T = TypeVar("T", bound="KvChoice")


@_attrs_define
class KvChoice:
    """An automatic choice.

    Attributes:
        level (KvLevel): User-facing precision levels ("Konuşma hafızası hassasiyeti").
        cache_type_k (str):
        cache_type_v (str):
        reason (KvReason): Why the automatic choice is what it is.
        kv_bytes_f16 (int): Whole KV cache with f16 and with this choice (bytes).
        kv_bytes (int):
        prompt_slowdown_pct (int | None | Unset): Typical prompt-processing slowdown of this choice vs f16 on the
            machine's backend
            (percent, measured; `None` = not measured for this backend).
    """

    level: KvLevel
    cache_type_k: str
    cache_type_v: str
    reason: KvReason
    kv_bytes_f16: int
    kv_bytes: int
    prompt_slowdown_pct: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        level: str = self.level

        cache_type_k = self.cache_type_k

        cache_type_v = self.cache_type_v

        reason: str = self.reason

        kv_bytes_f16 = self.kv_bytes_f16

        kv_bytes = self.kv_bytes

        prompt_slowdown_pct: int | None | Unset
        if isinstance(self.prompt_slowdown_pct, Unset):
            prompt_slowdown_pct = UNSET
        else:
            prompt_slowdown_pct = self.prompt_slowdown_pct

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "level": level,
                "cache_type_k": cache_type_k,
                "cache_type_v": cache_type_v,
                "reason": reason,
                "kv_bytes_f16": kv_bytes_f16,
                "kv_bytes": kv_bytes,
            }
        )
        if prompt_slowdown_pct is not UNSET:
            field_dict["prompt_slowdown_pct"] = prompt_slowdown_pct

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        level = check_kv_level(d.pop("level"))

        cache_type_k = d.pop("cache_type_k")

        cache_type_v = d.pop("cache_type_v")

        reason = check_kv_reason(d.pop("reason"))

        kv_bytes_f16 = d.pop("kv_bytes_f16")

        kv_bytes = d.pop("kv_bytes")

        def _parse_prompt_slowdown_pct(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        prompt_slowdown_pct = _parse_prompt_slowdown_pct(d.pop("prompt_slowdown_pct", UNSET))

        kv_choice = cls(
            level=level,
            cache_type_k=cache_type_k,
            cache_type_v=cache_type_v,
            reason=reason,
            kv_bytes_f16=kv_bytes_f16,
            kv_bytes=kv_bytes,
            prompt_slowdown_pct=prompt_slowdown_pct,
        )

        kv_choice.additional_properties = d
        return kv_choice

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
