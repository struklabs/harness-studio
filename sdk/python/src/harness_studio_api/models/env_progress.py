from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="EnvProgress")


@_attrs_define
class EnvProgress:
    """Progress of building an environment.

    Attributes:
        step (str): `uv`, `source`, `python`, `packages`, `verify`.
        detail (str): What is being fetched or the last line the tool printed.
        downloaded_bytes (int):
        total_bytes (int):
    """

    step: str
    detail: str
    downloaded_bytes: int
    total_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        step = self.step

        detail = self.detail

        downloaded_bytes = self.downloaded_bytes

        total_bytes = self.total_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "step": step,
                "detail": detail,
                "downloaded_bytes": downloaded_bytes,
                "total_bytes": total_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        step = d.pop("step")

        detail = d.pop("detail")

        downloaded_bytes = d.pop("downloaded_bytes")

        total_bytes = d.pop("total_bytes")

        env_progress = cls(
            step=step,
            detail=detail,
            downloaded_bytes=downloaded_bytes,
            total_bytes=total_bytes,
        )

        env_progress.additional_properties = d
        return env_progress

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
