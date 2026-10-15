from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.benchmark_run import BenchmarkRun
    from ..models.benchmark_status import BenchmarkStatus


T = TypeVar("T", bound="BenchmarkList")


@_attrs_define
class BenchmarkList:
    """
    Attributes:
        current (BenchmarkStatus):
        runs (list[BenchmarkRun]): Saved runs, newest first.
    """

    current: BenchmarkStatus
    runs: list[BenchmarkRun]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        current = self.current.to_dict()

        runs = []
        for runs_item_data in self.runs:
            runs_item = runs_item_data.to_dict()
            runs.append(runs_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "current": current,
                "runs": runs,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.benchmark_run import BenchmarkRun  # noqa: PLC0415
        from ..models.benchmark_status import BenchmarkStatus  # noqa: PLC0415

        d = dict(src_dict)
        current = BenchmarkStatus.from_dict(d.pop("current"))

        runs = []
        _runs = d.pop("runs")
        for runs_item_data in _runs:
            runs_item = BenchmarkRun.from_dict(runs_item_data)

            runs.append(runs_item)

        benchmark_list = cls(
            current=current,
            runs=runs,
        )

        benchmark_list.additional_properties = d
        return benchmark_list

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
