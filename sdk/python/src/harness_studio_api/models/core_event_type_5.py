from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_5_type import (
    CoreEventType5Type,
    check_core_event_type_5_type,
)

if TYPE_CHECKING:
    from ..models.convert_job import ConvertJob


T = TypeVar("T", bound="CoreEventType5")


@_attrs_define
class CoreEventType5:
    """A safetensors to GGUF conversion job was queued, changed stage or progressed.

    Attributes:
        job (ConvertJob): One conversion, as the queue shows it.
        type_ (CoreEventType5Type):
    """

    job: ConvertJob
    type_: CoreEventType5Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        job = self.job.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "job": job,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.convert_job import ConvertJob  # noqa: PLC0415

        d = dict(src_dict)
        job = ConvertJob.from_dict(d.pop("job"))

        type_ = check_core_event_type_5_type(d.pop("type"))

        core_event_type_5 = cls(
            job=job,
            type_=type_,
        )

        core_event_type_5.additional_properties = d
        return core_event_type_5

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
