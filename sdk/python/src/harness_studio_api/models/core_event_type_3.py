from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_3_type import (
    CoreEventType3Type,
    check_core_event_type_3_type,
)

if TYPE_CHECKING:
    from ..models.download_progress import DownloadProgress


T = TypeVar("T", bound="CoreEventType3")


@_attrs_define
class CoreEventType3:
    """A download job was queued, progressed (at most ~2/s per job) or changed status.

    Attributes:
        progress (DownloadProgress):
        type_ (CoreEventType3Type):
    """

    progress: DownloadProgress
    type_: CoreEventType3Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        progress = self.progress.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "progress": progress,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.download_progress import DownloadProgress  # noqa: PLC0415

        d = dict(src_dict)
        progress = DownloadProgress.from_dict(d.pop("progress"))

        type_ = check_core_event_type_3_type(d.pop("type"))

        core_event_type_3 = cls(
            progress=progress,
            type_=type_,
        )

        core_event_type_3.additional_properties = d
        return core_event_type_3

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
