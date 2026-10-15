from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ClientShare")


@_attrs_define
class ClientShare:
    """
    Attributes:
        client (str): Client label (empty when unknown).
        requests (int):
        prompt_tokens (int):
        completion_tokens (int):
        total_tokens (int):
        share (float): Share of all requests in the range (0–1).
        token_share (float): Share of all tokens in the range (0–1).
    """

    client: str
    requests: int
    prompt_tokens: int
    completion_tokens: int
    total_tokens: int
    share: float
    token_share: float
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        client = self.client

        requests = self.requests

        prompt_tokens = self.prompt_tokens

        completion_tokens = self.completion_tokens

        total_tokens = self.total_tokens

        share = self.share

        token_share = self.token_share

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "client": client,
                "requests": requests,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": total_tokens,
                "share": share,
                "token_share": token_share,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        client = d.pop("client")

        requests = d.pop("requests")

        prompt_tokens = d.pop("prompt_tokens")

        completion_tokens = d.pop("completion_tokens")

        total_tokens = d.pop("total_tokens")

        share = d.pop("share")

        token_share = d.pop("token_share")

        client_share = cls(
            client=client,
            requests=requests,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            total_tokens=total_tokens,
            share=share,
            token_share=token_share,
        )

        client_share.additional_properties = d
        return client_share

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
