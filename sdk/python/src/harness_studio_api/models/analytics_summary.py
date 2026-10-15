from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.client_share import ClientShare
    from ..models.daily_point import DailyPoint
    from ..models.direct_usage import DirectUsage
    from ..models.model_stats import ModelStats
    from ..models.recorder_status import RecorderStatus


T = TypeVar("T", bound="AnalyticsSummary")


@_attrs_define
class AnalyticsSummary:
    """`GET /analytics/summary`.

    Attributes:
        generated_at_ms (int):
        totals (ModelStats): Statistics of one model (alias) over a time range.
        models (list[ModelStats]): Per gateway model, most requests first.
        clients (list[ClientShare]):
        daily (list[DailyPoint]):
        rollup_days (int): Days in the range that only survive as daily rollups (older than the raw retention).
        retention_days (int):
        recorder (RecorderStatus): Counters of the recorder since this process started.
        from_ms (int | None | Unset):
        to_ms (int | None | Unset):
        raw_since_ms (int | None | Unset): Oldest raw row still stored; percentiles only cover raw rows.
        direct (DirectUsage | Unset): Direct usage in the range of an analytics summary: aggregate token totals only, no
            per-request detail. See the module docs for how it is derived.
    """

    generated_at_ms: int
    totals: ModelStats
    models: list[ModelStats]
    clients: list[ClientShare]
    daily: list[DailyPoint]
    rollup_days: int
    retention_days: int
    recorder: RecorderStatus
    from_ms: int | None | Unset = UNSET
    to_ms: int | None | Unset = UNSET
    raw_since_ms: int | None | Unset = UNSET
    direct: DirectUsage | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        generated_at_ms = self.generated_at_ms

        totals = self.totals.to_dict()

        models = []
        for models_item_data in self.models:
            models_item = models_item_data.to_dict()
            models.append(models_item)

        clients = []
        for clients_item_data in self.clients:
            clients_item = clients_item_data.to_dict()
            clients.append(clients_item)

        daily = []
        for daily_item_data in self.daily:
            daily_item = daily_item_data.to_dict()
            daily.append(daily_item)

        rollup_days = self.rollup_days

        retention_days = self.retention_days

        recorder = self.recorder.to_dict()

        from_ms: int | None | Unset
        if isinstance(self.from_ms, Unset):
            from_ms = UNSET
        else:
            from_ms = self.from_ms

        to_ms: int | None | Unset
        if isinstance(self.to_ms, Unset):
            to_ms = UNSET
        else:
            to_ms = self.to_ms

        raw_since_ms: int | None | Unset
        if isinstance(self.raw_since_ms, Unset):
            raw_since_ms = UNSET
        else:
            raw_since_ms = self.raw_since_ms

        direct: dict[str, Any] | Unset = UNSET
        if not isinstance(self.direct, Unset):
            direct = self.direct.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "generated_at_ms": generated_at_ms,
                "totals": totals,
                "models": models,
                "clients": clients,
                "daily": daily,
                "rollup_days": rollup_days,
                "retention_days": retention_days,
                "recorder": recorder,
            }
        )
        if from_ms is not UNSET:
            field_dict["from_ms"] = from_ms
        if to_ms is not UNSET:
            field_dict["to_ms"] = to_ms
        if raw_since_ms is not UNSET:
            field_dict["raw_since_ms"] = raw_since_ms
        if direct is not UNSET:
            field_dict["direct"] = direct

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.client_share import ClientShare  # noqa: PLC0415
        from ..models.daily_point import DailyPoint  # noqa: PLC0415
        from ..models.direct_usage import DirectUsage  # noqa: PLC0415
        from ..models.model_stats import ModelStats  # noqa: PLC0415
        from ..models.recorder_status import RecorderStatus  # noqa: PLC0415

        d = dict(src_dict)
        generated_at_ms = d.pop("generated_at_ms")

        totals = ModelStats.from_dict(d.pop("totals"))

        models = []
        _models = d.pop("models")
        for models_item_data in _models:
            models_item = ModelStats.from_dict(models_item_data)

            models.append(models_item)

        clients = []
        _clients = d.pop("clients")
        for clients_item_data in _clients:
            clients_item = ClientShare.from_dict(clients_item_data)

            clients.append(clients_item)

        daily = []
        _daily = d.pop("daily")
        for daily_item_data in _daily:
            daily_item = DailyPoint.from_dict(daily_item_data)

            daily.append(daily_item)

        rollup_days = d.pop("rollup_days")

        retention_days = d.pop("retention_days")

        recorder = RecorderStatus.from_dict(d.pop("recorder"))

        def _parse_from_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        from_ms = _parse_from_ms(d.pop("from_ms", UNSET))

        def _parse_to_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        to_ms = _parse_to_ms(d.pop("to_ms", UNSET))

        def _parse_raw_since_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        raw_since_ms = _parse_raw_since_ms(d.pop("raw_since_ms", UNSET))

        _direct = d.pop("direct", UNSET)
        direct: DirectUsage | Unset
        if isinstance(_direct, Unset):
            direct = UNSET
        else:
            direct = DirectUsage.from_dict(_direct)

        analytics_summary = cls(
            generated_at_ms=generated_at_ms,
            totals=totals,
            models=models,
            clients=clients,
            daily=daily,
            rollup_days=rollup_days,
            retention_days=retention_days,
            recorder=recorder,
            from_ms=from_ms,
            to_ms=to_ms,
            raw_since_ms=raw_since_ms,
            direct=direct,
        )

        analytics_summary.additional_properties = d
        return analytics_summary

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
