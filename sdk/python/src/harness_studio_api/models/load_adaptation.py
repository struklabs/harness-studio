from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="LoadAdaptation")


@_attrs_define
class LoadAdaptation:
    """Cache types and context the gateway loaded a model with to fit it into the free memory
    (the definition asked for more; it stays as the user left it).

        Attributes:
            cache_type_k (str):
            cache_type_v (str):
            context_size (int):
            requested_context (int): The context the definition asked for.
            gpu_layers (int | None | Unset): Layers left on the GPU when even the smallest context did not fit the whole
                model
                (automatic `-ngl` only); `None` = the recommended count.
    """

    cache_type_k: str
    cache_type_v: str
    context_size: int
    requested_context: int
    gpu_layers: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        cache_type_k = self.cache_type_k

        cache_type_v = self.cache_type_v

        context_size = self.context_size

        requested_context = self.requested_context

        gpu_layers: int | None | Unset
        if isinstance(self.gpu_layers, Unset):
            gpu_layers = UNSET
        else:
            gpu_layers = self.gpu_layers

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "cache_type_k": cache_type_k,
                "cache_type_v": cache_type_v,
                "context_size": context_size,
                "requested_context": requested_context,
            }
        )
        if gpu_layers is not UNSET:
            field_dict["gpu_layers"] = gpu_layers

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        cache_type_k = d.pop("cache_type_k")

        cache_type_v = d.pop("cache_type_v")

        context_size = d.pop("context_size")

        requested_context = d.pop("requested_context")

        def _parse_gpu_layers(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        gpu_layers = _parse_gpu_layers(d.pop("gpu_layers", UNSET))

        load_adaptation = cls(
            cache_type_k=cache_type_k,
            cache_type_v=cache_type_v,
            context_size=context_size,
            requested_context=requested_context,
            gpu_layers=gpu_layers,
        )

        load_adaptation.additional_properties = d
        return load_adaptation

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
