from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="TelemetryHistoryPoint")


@_attrs_define
class TelemetryHistoryPoint:
    """
    Attributes:
        timestamp (int):
        predicted_tokens_per_sec (float):
        prompt_tokens_per_sec (float):
        vram_used_bytes (int):
        vram_total_bytes (int):
        ram_used_bytes (int):
        ram_total_bytes (int):
        active_slots (int):
        total_slots (int):
        requests_processing (int):
        requests_deferred (int):
        server_vram_used_bytes (int | None | Unset): This server's dedicated VRAM across adapters; None without a
            breakdown.
        server_ram_used_bytes (int | None | Unset): This server's RAM share; None without a breakdown.
        kv_usage_ratio (float | None | Unset): Share of the context window in use; None when the server exposes no such
            data.
    """

    timestamp: int
    predicted_tokens_per_sec: float
    prompt_tokens_per_sec: float
    vram_used_bytes: int
    vram_total_bytes: int
    ram_used_bytes: int
    ram_total_bytes: int
    active_slots: int
    total_slots: int
    requests_processing: int
    requests_deferred: int
    server_vram_used_bytes: int | None | Unset = UNSET
    server_ram_used_bytes: int | None | Unset = UNSET
    kv_usage_ratio: float | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        timestamp = self.timestamp

        predicted_tokens_per_sec = self.predicted_tokens_per_sec

        prompt_tokens_per_sec = self.prompt_tokens_per_sec

        vram_used_bytes = self.vram_used_bytes

        vram_total_bytes = self.vram_total_bytes

        ram_used_bytes = self.ram_used_bytes

        ram_total_bytes = self.ram_total_bytes

        active_slots = self.active_slots

        total_slots = self.total_slots

        requests_processing = self.requests_processing

        requests_deferred = self.requests_deferred

        server_vram_used_bytes: int | None | Unset
        if isinstance(self.server_vram_used_bytes, Unset):
            server_vram_used_bytes = UNSET
        else:
            server_vram_used_bytes = self.server_vram_used_bytes

        server_ram_used_bytes: int | None | Unset
        if isinstance(self.server_ram_used_bytes, Unset):
            server_ram_used_bytes = UNSET
        else:
            server_ram_used_bytes = self.server_ram_used_bytes

        kv_usage_ratio: float | None | Unset
        if isinstance(self.kv_usage_ratio, Unset):
            kv_usage_ratio = UNSET
        else:
            kv_usage_ratio = self.kv_usage_ratio

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "timestamp": timestamp,
                "predicted_tokens_per_sec": predicted_tokens_per_sec,
                "prompt_tokens_per_sec": prompt_tokens_per_sec,
                "vram_used_bytes": vram_used_bytes,
                "vram_total_bytes": vram_total_bytes,
                "ram_used_bytes": ram_used_bytes,
                "ram_total_bytes": ram_total_bytes,
                "active_slots": active_slots,
                "total_slots": total_slots,
                "requests_processing": requests_processing,
                "requests_deferred": requests_deferred,
            }
        )
        if server_vram_used_bytes is not UNSET:
            field_dict["server_vram_used_bytes"] = server_vram_used_bytes
        if server_ram_used_bytes is not UNSET:
            field_dict["server_ram_used_bytes"] = server_ram_used_bytes
        if kv_usage_ratio is not UNSET:
            field_dict["kv_usage_ratio"] = kv_usage_ratio

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        timestamp = d.pop("timestamp")

        predicted_tokens_per_sec = d.pop("predicted_tokens_per_sec")

        prompt_tokens_per_sec = d.pop("prompt_tokens_per_sec")

        vram_used_bytes = d.pop("vram_used_bytes")

        vram_total_bytes = d.pop("vram_total_bytes")

        ram_used_bytes = d.pop("ram_used_bytes")

        ram_total_bytes = d.pop("ram_total_bytes")

        active_slots = d.pop("active_slots")

        total_slots = d.pop("total_slots")

        requests_processing = d.pop("requests_processing")

        requests_deferred = d.pop("requests_deferred")

        def _parse_server_vram_used_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        server_vram_used_bytes = _parse_server_vram_used_bytes(
            d.pop("server_vram_used_bytes", UNSET)
        )

        def _parse_server_ram_used_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        server_ram_used_bytes = _parse_server_ram_used_bytes(d.pop("server_ram_used_bytes", UNSET))

        def _parse_kv_usage_ratio(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        kv_usage_ratio = _parse_kv_usage_ratio(d.pop("kv_usage_ratio", UNSET))

        telemetry_history_point = cls(
            timestamp=timestamp,
            predicted_tokens_per_sec=predicted_tokens_per_sec,
            prompt_tokens_per_sec=prompt_tokens_per_sec,
            vram_used_bytes=vram_used_bytes,
            vram_total_bytes=vram_total_bytes,
            ram_used_bytes=ram_used_bytes,
            ram_total_bytes=ram_total_bytes,
            active_slots=active_slots,
            total_slots=total_slots,
            requests_processing=requests_processing,
            requests_deferred=requests_deferred,
            server_vram_used_bytes=server_vram_used_bytes,
            server_ram_used_bytes=server_ram_used_bytes,
            kv_usage_ratio=kv_usage_ratio,
        )

        telemetry_history_point.additional_properties = d
        return telemetry_history_point

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
