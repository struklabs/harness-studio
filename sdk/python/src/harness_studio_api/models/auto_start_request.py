from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.auto_start_request_endpoint import AutoStartRequestEndpoint


T = TypeVar("T", bound="AutoStartRequest")


@_attrs_define
class AutoStartRequest:
    """
    Attributes:
        endpoint (AutoStartRequestEndpoint):
        count (int | Unset):
        resume (bool | Unset):
    """

    endpoint: AutoStartRequestEndpoint
    count: int | Unset = UNSET
    resume: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        endpoint = self.endpoint.to_dict()

        count = self.count

        resume = self.resume

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "endpoint": endpoint,
            }
        )
        if count is not UNSET:
            field_dict["count"] = count
        if resume is not UNSET:
            field_dict["resume"] = resume

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.auto_start_request_endpoint import (
            AutoStartRequestEndpoint,  # noqa: PLC0415
        )

        d = dict(src_dict)
        endpoint = AutoStartRequestEndpoint.from_dict(d.pop("endpoint"))

        count = d.pop("count", UNSET)

        resume = d.pop("resume", UNSET)

        auto_start_request = cls(
            endpoint=endpoint,
            count=count,
            resume=resume,
        )

        auto_start_request.additional_properties = d
        return auto_start_request

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
