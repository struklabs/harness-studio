from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="StoredContent")


@_attrs_define
class StoredContent:
    """Stored content of one request (Request inspector).

    Attributes:
        captured_at_ms (int):
        expires_at_ms (int):
        request (str): Request body (JSON as sent, cleaned).
        response (str): Response as received (JSON, or the SSE event stream), cleaned.
        request_truncated (bool):
        response_truncated (bool):
    """

    captured_at_ms: int
    expires_at_ms: int
    request: str
    response: str
    request_truncated: bool
    response_truncated: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        captured_at_ms = self.captured_at_ms

        expires_at_ms = self.expires_at_ms

        request = self.request

        response = self.response

        request_truncated = self.request_truncated

        response_truncated = self.response_truncated

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "captured_at_ms": captured_at_ms,
                "expires_at_ms": expires_at_ms,
                "request": request,
                "response": response,
                "request_truncated": request_truncated,
                "response_truncated": response_truncated,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        captured_at_ms = d.pop("captured_at_ms")

        expires_at_ms = d.pop("expires_at_ms")

        request = d.pop("request")

        response = d.pop("response")

        request_truncated = d.pop("request_truncated")

        response_truncated = d.pop("response_truncated")

        stored_content = cls(
            captured_at_ms=captured_at_ms,
            expires_at_ms=expires_at_ms,
            request=request,
            response=response,
            request_truncated=request_truncated,
            response_truncated=response_truncated,
        )

        stored_content.additional_properties = d
        return stored_content

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
