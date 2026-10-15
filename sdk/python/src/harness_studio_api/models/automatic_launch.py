from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AutomaticLaunch")


@_attrs_define
class AutomaticLaunch:
    """Everything an automatic default load starts with, for forms and `harness show` that display it
    before Start: the full-offload choice plus the slot count and the KV cache the launch ends
    up with (the cache types and the flash attention they need).

        Attributes:
            context (int):
            layers (int):
            flash_attn (bool): A quantized V cache needs flash attention: the launch turns it on.
            slots (int | None | Unset): Concurrent requests (`-np`) the launch runs with; `None` when no slot plan applies.
            cache_type_k (None | str | Unset): Cache types of the automatic KV precision (`f16` for the full level).
            cache_type_v (None | str | Unset):
    """

    context: int
    layers: int
    flash_attn: bool
    slots: int | None | Unset = UNSET
    cache_type_k: None | str | Unset = UNSET
    cache_type_v: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        context = self.context

        layers = self.layers

        flash_attn = self.flash_attn

        slots: int | None | Unset
        if isinstance(self.slots, Unset):
            slots = UNSET
        else:
            slots = self.slots

        cache_type_k: None | str | Unset
        if isinstance(self.cache_type_k, Unset):
            cache_type_k = UNSET
        else:
            cache_type_k = self.cache_type_k

        cache_type_v: None | str | Unset
        if isinstance(self.cache_type_v, Unset):
            cache_type_v = UNSET
        else:
            cache_type_v = self.cache_type_v

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "context": context,
                "layers": layers,
                "flash_attn": flash_attn,
            }
        )
        if slots is not UNSET:
            field_dict["slots"] = slots
        if cache_type_k is not UNSET:
            field_dict["cache_type_k"] = cache_type_k
        if cache_type_v is not UNSET:
            field_dict["cache_type_v"] = cache_type_v

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        context = d.pop("context")

        layers = d.pop("layers")

        flash_attn = d.pop("flash_attn")

        def _parse_slots(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        slots = _parse_slots(d.pop("slots", UNSET))

        def _parse_cache_type_k(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cache_type_k = _parse_cache_type_k(d.pop("cache_type_k", UNSET))

        def _parse_cache_type_v(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cache_type_v = _parse_cache_type_v(d.pop("cache_type_v", UNSET))

        automatic_launch = cls(
            context=context,
            layers=layers,
            flash_attn=flash_attn,
            slots=slots,
            cache_type_k=cache_type_k,
            cache_type_v=cache_type_v,
        )

        automatic_launch.additional_properties = d
        return automatic_launch

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
