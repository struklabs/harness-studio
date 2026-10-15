from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.telemetry_history_point import TelemetryHistoryPoint
    from ..models.telemetry_snapshot import TelemetrySnapshot


T = TypeVar("T", bound="MetricsResponse")


@_attrs_define
class MetricsResponse:
    """
    Attributes:
        snapshot (TelemetrySnapshot):
        history (list[TelemetryHistoryPoint]):
    """

    snapshot: TelemetrySnapshot
    history: list[TelemetryHistoryPoint]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        snapshot = self.snapshot.to_dict()

        history = []
        for history_item_data in self.history:
            history_item = history_item_data.to_dict()
            history.append(history_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "snapshot": snapshot,
                "history": history,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.telemetry_history_point import (
            TelemetryHistoryPoint,  # noqa: PLC0415
        )
        from ..models.telemetry_snapshot import TelemetrySnapshot  # noqa: PLC0415

        d = dict(src_dict)
        snapshot = TelemetrySnapshot.from_dict(d.pop("snapshot"))

        history = []
        _history = d.pop("history")
        for history_item_data in _history:
            history_item = TelemetryHistoryPoint.from_dict(history_item_data)

            history.append(history_item)

        metrics_response = cls(
            snapshot=snapshot,
            history=history,
        )

        metrics_response.additional_properties = d
        return metrics_response

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
