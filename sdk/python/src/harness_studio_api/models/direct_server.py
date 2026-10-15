from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="DirectServer")


@_attrs_define
class DirectServer:
    """Tokens of one server run in a range.

    Attributes:
        server_id (str):
        label (str): What the server ran (model file name).
        prompt_tokens (int):
        completion_tokens (int):
    """

    server_id: str
    label: str
    prompt_tokens: int
    completion_tokens: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        label = self.label

        prompt_tokens = self.prompt_tokens

        completion_tokens = self.completion_tokens

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_id": server_id,
                "label": label,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        server_id = d.pop("server_id")

        label = d.pop("label")

        prompt_tokens = d.pop("prompt_tokens")

        completion_tokens = d.pop("completion_tokens")

        direct_server = cls(
            server_id=server_id,
            label=label,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
        )

        direct_server.additional_properties = d
        return direct_server

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
