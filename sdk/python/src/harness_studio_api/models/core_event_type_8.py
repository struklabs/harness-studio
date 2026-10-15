from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_8_type import (
    CoreEventType8Type,
    check_core_event_type_8_type,
)

if TYPE_CHECKING:
    from ..models.benchmark_run import BenchmarkRun


T = TypeVar("T", bound="CoreEventType8")


@_attrs_define
class CoreEventType8:
    """A llama-bench run finished (completed, cancelled or failed).

    Attributes:
        run (BenchmarkRun):
        type_ (CoreEventType8Type):
    """

    run: BenchmarkRun
    type_: CoreEventType8Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        run = self.run.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "run": run,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.benchmark_run import BenchmarkRun  # noqa: PLC0415

        d = dict(src_dict)
        run = BenchmarkRun.from_dict(d.pop("run"))

        type_ = check_core_event_type_8_type(d.pop("type"))

        core_event_type_8 = cls(
            run=run,
            type_=type_,
        )

        core_event_type_8.additional_properties = d
        return core_event_type_8

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
