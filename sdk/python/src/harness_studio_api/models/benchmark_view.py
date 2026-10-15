from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.benchmark_run import BenchmarkRun
    from ..models.benchmark_status import BenchmarkStatus


T = TypeVar("T", bound="BenchmarkView")


@_attrs_define
class BenchmarkView:
    """A run: live while it runs, from the history afterwards.

    Attributes:
        id (str):
        running (bool):
        status (BenchmarkStatus | None | Unset):
        run (BenchmarkRun | None | Unset):
    """

    id: str
    running: bool
    status: BenchmarkStatus | None | Unset = UNSET
    run: BenchmarkRun | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.benchmark_run import BenchmarkRun  # noqa: PLC0415
        from ..models.benchmark_status import BenchmarkStatus  # noqa: PLC0415

        id = self.id

        running = self.running

        status: dict[str, Any] | None | Unset
        if isinstance(self.status, Unset):
            status = UNSET
        elif isinstance(self.status, BenchmarkStatus):
            status = self.status.to_dict()
        else:
            status = self.status

        run: dict[str, Any] | None | Unset
        if isinstance(self.run, Unset):
            run = UNSET
        elif isinstance(self.run, BenchmarkRun):
            run = self.run.to_dict()
        else:
            run = self.run

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "running": running,
            }
        )
        if status is not UNSET:
            field_dict["status"] = status
        if run is not UNSET:
            field_dict["run"] = run

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.benchmark_run import BenchmarkRun  # noqa: PLC0415
        from ..models.benchmark_status import BenchmarkStatus  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        running = d.pop("running")

        def _parse_status(data: object) -> BenchmarkStatus | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                status_type_1 = BenchmarkStatus.from_dict(data)

                return status_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BenchmarkStatus | None | Unset, data)

        status = _parse_status(d.pop("status", UNSET))

        def _parse_run(data: object) -> BenchmarkRun | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                run_type_1 = BenchmarkRun.from_dict(data)

                return run_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BenchmarkRun | None | Unset, data)

        run = _parse_run(d.pop("run", UNSET))

        benchmark_view = cls(
            id=id,
            running=running,
            status=status,
            run=run,
        )

        benchmark_view.additional_properties = d
        return benchmark_view

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
