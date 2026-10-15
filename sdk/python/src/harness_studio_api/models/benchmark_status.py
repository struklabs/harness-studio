from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.bench_result import BenchResult
    from ..models.benchmark_run import BenchmarkRun


T = TypeVar("T", bound="BenchmarkStatus")


@_attrs_define
class BenchmarkStatus:
    """
    Attributes:
        running (bool):
        current (int):
        total (int):
        phase (str): Latest progress line, e.g. "generation run 2/5".
        results (list[BenchResult]):
        log_tail (list[str]):
        run_id (None | str | Unset): Id of the current (or latest) run; the saved run gets the same id.
        last_run (BenchmarkRun | None | Unset):
    """

    running: bool
    current: int
    total: int
    phase: str
    results: list[BenchResult]
    log_tail: list[str]
    run_id: None | str | Unset = UNSET
    last_run: BenchmarkRun | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.benchmark_run import BenchmarkRun  # noqa: PLC0415

        running = self.running

        current = self.current

        total = self.total

        phase = self.phase

        results = []
        for results_item_data in self.results:
            results_item = results_item_data.to_dict()
            results.append(results_item)

        log_tail = self.log_tail

        run_id: None | str | Unset
        if isinstance(self.run_id, Unset):
            run_id = UNSET
        else:
            run_id = self.run_id

        last_run: dict[str, Any] | None | Unset
        if isinstance(self.last_run, Unset):
            last_run = UNSET
        elif isinstance(self.last_run, BenchmarkRun):
            last_run = self.last_run.to_dict()
        else:
            last_run = self.last_run

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "running": running,
                "current": current,
                "total": total,
                "phase": phase,
                "results": results,
                "log_tail": log_tail,
            }
        )
        if run_id is not UNSET:
            field_dict["run_id"] = run_id
        if last_run is not UNSET:
            field_dict["last_run"] = last_run

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bench_result import BenchResult  # noqa: PLC0415
        from ..models.benchmark_run import BenchmarkRun  # noqa: PLC0415

        d = dict(src_dict)
        running = d.pop("running")

        current = d.pop("current")

        total = d.pop("total")

        phase = d.pop("phase")

        results = []
        _results = d.pop("results")
        for results_item_data in _results:
            results_item = BenchResult.from_dict(results_item_data)

            results.append(results_item)

        log_tail = cast(list[str], d.pop("log_tail"))

        def _parse_run_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        run_id = _parse_run_id(d.pop("run_id", UNSET))

        def _parse_last_run(data: object) -> BenchmarkRun | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                last_run_type_1 = BenchmarkRun.from_dict(data)

                return last_run_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BenchmarkRun | None | Unset, data)

        last_run = _parse_last_run(d.pop("last_run", UNSET))

        benchmark_status = cls(
            running=running,
            current=current,
            total=total,
            phase=phase,
            results=results,
            log_tail=log_tail,
            run_id=run_id,
            last_run=last_run,
        )

        benchmark_status.additional_properties = d
        return benchmark_status

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
