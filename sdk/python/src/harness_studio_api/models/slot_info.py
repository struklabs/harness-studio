from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="SlotInfo")


@_attrs_define
class SlotInfo:
    """One slot as reported by /slots.

    Attributes:
        id (int):
        busy (bool):
        prompt_tokens (int):
        decoded_tokens (int):
    """

    id: int
    busy: bool
    prompt_tokens: int
    decoded_tokens: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        busy = self.busy

        prompt_tokens = self.prompt_tokens

        decoded_tokens = self.decoded_tokens

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "busy": busy,
                "prompt_tokens": prompt_tokens,
                "decoded_tokens": decoded_tokens,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        busy = d.pop("busy")

        prompt_tokens = d.pop("prompt_tokens")

        decoded_tokens = d.pop("decoded_tokens")

        slot_info = cls(
            id=id,
            busy=busy,
            prompt_tokens=prompt_tokens,
            decoded_tokens=decoded_tokens,
        )

        slot_info.additional_properties = d
        return slot_info

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
