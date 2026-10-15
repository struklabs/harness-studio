from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.run_status import RunStatus, check_run_status
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.bench_result import BenchResult
    from ..models.benchmark_request import BenchmarkRequest


T = TypeVar("T", bound="BenchmarkRun")


@_attrs_define
class BenchmarkRun:
    """
    Attributes:
        id (str):
        started_at (int):
        finished_at (int):
        model_name (str):
        request (BenchmarkRequest):
        results (list[BenchResult]):
        status (RunStatus):
        error (None | str | Unset):
    """

    id: str
    started_at: int
    finished_at: int
    model_name: str
    request: BenchmarkRequest
    results: list[BenchResult]
    status: RunStatus
    error: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        started_at = self.started_at

        finished_at = self.finished_at

        model_name = self.model_name

        request = self.request.to_dict()

        results = []
        for results_item_data in self.results:
            results_item = results_item_data.to_dict()
            results.append(results_item)

        status: str = self.status

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "started_at": started_at,
                "finished_at": finished_at,
                "model_name": model_name,
                "request": request,
                "results": results,
                "status": status,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.bench_result import BenchResult  # noqa: PLC0415
        from ..models.benchmark_request import BenchmarkRequest  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        started_at = d.pop("started_at")

        finished_at = d.pop("finished_at")

        model_name = d.pop("model_name")

        request = BenchmarkRequest.from_dict(d.pop("request"))

        results = []
        _results = d.pop("results")
        for results_item_data in _results:
            results_item = BenchResult.from_dict(results_item_data)

            results.append(results_item)

        status = check_run_status(d.pop("status"))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        benchmark_run = cls(
            id=id,
            started_at=started_at,
            finished_at=finished_at,
            model_name=model_name,
            request=request,
            results=results,
            status=status,
            error=error,
        )

        benchmark_run.additional_properties = d
        return benchmark_run

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
