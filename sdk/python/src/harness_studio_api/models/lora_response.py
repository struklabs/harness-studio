from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.lora_adapter import LoraAdapter


T = TypeVar("T", bound="LoraResponse")


@_attrs_define
class LoraResponse:
    """
    Attributes:
        adapters (list[LoraAdapter]): The adapters of the running server with their current scales.
    """

    adapters: list[LoraAdapter]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        adapters = []
        for adapters_item_data in self.adapters:
            adapters_item = adapters_item_data.to_dict()
            adapters.append(adapters_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "adapters": adapters,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.lora_adapter import LoraAdapter  # noqa: PLC0415

        d = dict(src_dict)
        adapters = []
        _adapters = d.pop("adapters")
        for adapters_item_data in _adapters:
            adapters_item = LoraAdapter.from_dict(adapters_item_data)

            adapters.append(adapters_item)

        lora_response = cls(
            adapters=adapters,
        )

        lora_response.additional_properties = d
        return lora_response

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
