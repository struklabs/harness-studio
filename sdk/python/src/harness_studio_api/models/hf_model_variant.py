from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.hf_shard_info import HfShardInfo


T = TypeVar("T", bound="HfModelVariant")


@_attrs_define
class HfModelVariant:
    """
    Attributes:
        name (str):
        quantization (str):
        total_size_bytes (int):
        is_split (bool):
        shards (list[HfShardInfo]):
        not_runnable (None | str | Unset):
    """

    name: str
    quantization: str
    total_size_bytes: int
    is_split: bool
    shards: list[HfShardInfo]
    not_runnable: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        name = self.name

        quantization = self.quantization

        total_size_bytes = self.total_size_bytes

        is_split = self.is_split

        shards = []
        for shards_item_data in self.shards:
            shards_item = shards_item_data.to_dict()
            shards.append(shards_item)

        not_runnable: None | str | Unset
        if isinstance(self.not_runnable, Unset):
            not_runnable = UNSET
        else:
            not_runnable = self.not_runnable

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "name": name,
                "quantization": quantization,
                "total_size_bytes": total_size_bytes,
                "is_split": is_split,
                "shards": shards,
            }
        )
        if not_runnable is not UNSET:
            field_dict["not_runnable"] = not_runnable

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.hf_shard_info import HfShardInfo  # noqa: PLC0415

        d = dict(src_dict)
        name = d.pop("name")

        quantization = d.pop("quantization")

        total_size_bytes = d.pop("total_size_bytes")

        is_split = d.pop("is_split")

        shards = []
        _shards = d.pop("shards")
        for shards_item_data in _shards:
            shards_item = HfShardInfo.from_dict(shards_item_data)

            shards.append(shards_item)

        def _parse_not_runnable(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        not_runnable = _parse_not_runnable(d.pop("not_runnable", UNSET))

        hf_model_variant = cls(
            name=name,
            quantization=quantization,
            total_size_bytes=total_size_bytes,
            is_split=is_split,
            shards=shards,
            not_runnable=not_runnable,
        )

        hf_model_variant.additional_properties = d
        return hf_model_variant

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
