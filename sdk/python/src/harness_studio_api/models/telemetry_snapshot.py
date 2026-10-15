from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.gpu_memory import GpuMemory
    from ..models.last_request import LastRequest
    from ..models.memory_breakdown import MemoryBreakdown
    from ..models.server_totals import ServerTotals
    from ..models.slot_info import SlotInfo


T = TypeVar("T", bound="TelemetrySnapshot")


@_attrs_define
class TelemetrySnapshot:
    """
    Attributes:
        server_online (bool):
        predicted_tokens_per_sec (float): Current generation speed (from counter deltas over the last sample; 0 when
            idle).
        prompt_tokens_per_sec (float): Current prompt processing speed (from counter deltas; 0 when idle).
        avg_predicted_tokens_per_sec (float): Server-reported averages since start.
        avg_prompt_tokens_per_sec (float):
        kv_cache_usage_ratio (float): KV cache usage ratio, when the server still exposes it (older builds).
        kv_cache_available (bool):
        active_slots (int):
        total_slots (int):
        requests_processing (int):
        requests_deferred (int):
        system_ram_used_bytes (int):
        system_ram_total_bytes (int):
        vram_used_bytes (int):
        vram_total_bytes (int):
        gpus (list[GpuMemory]): Every graphics adapter with its own VRAM and split, in adapter order (the first is the
            one `vram_*` above describe). Empty with a single adapter: the fields above are all.
        slots (list[SlotInfo]): Per-slot state, in slot order.
        spec_acceptance_rate (float | None | Unset): Speculative decoding: accepted / drafted tokens (None when not
            drafting).
        ram_breakdown (MemoryBreakdown | None | Unset):
        vram_breakdown (MemoryBreakdown | None | Unset):
        context_size (int | None | Unset): Context window size and tokens held by busy slots (from /slots); None without
            slot data.
        context_used_tokens (int | None | Unset):
        totals (None | ServerTotals | Unset):
        last_request (LastRequest | None | Unset):
    """

    server_online: bool
    predicted_tokens_per_sec: float
    prompt_tokens_per_sec: float
    avg_predicted_tokens_per_sec: float
    avg_prompt_tokens_per_sec: float
    kv_cache_usage_ratio: float
    kv_cache_available: bool
    active_slots: int
    total_slots: int
    requests_processing: int
    requests_deferred: int
    system_ram_used_bytes: int
    system_ram_total_bytes: int
    vram_used_bytes: int
    vram_total_bytes: int
    gpus: list[GpuMemory]
    slots: list[SlotInfo]
    spec_acceptance_rate: float | None | Unset = UNSET
    ram_breakdown: MemoryBreakdown | None | Unset = UNSET
    vram_breakdown: MemoryBreakdown | None | Unset = UNSET
    context_size: int | None | Unset = UNSET
    context_used_tokens: int | None | Unset = UNSET
    totals: None | ServerTotals | Unset = UNSET
    last_request: LastRequest | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.last_request import LastRequest  # noqa: PLC0415
        from ..models.memory_breakdown import MemoryBreakdown  # noqa: PLC0415
        from ..models.server_totals import ServerTotals  # noqa: PLC0415

        server_online = self.server_online

        predicted_tokens_per_sec = self.predicted_tokens_per_sec

        prompt_tokens_per_sec = self.prompt_tokens_per_sec

        avg_predicted_tokens_per_sec = self.avg_predicted_tokens_per_sec

        avg_prompt_tokens_per_sec = self.avg_prompt_tokens_per_sec

        kv_cache_usage_ratio = self.kv_cache_usage_ratio

        kv_cache_available = self.kv_cache_available

        active_slots = self.active_slots

        total_slots = self.total_slots

        requests_processing = self.requests_processing

        requests_deferred = self.requests_deferred

        system_ram_used_bytes = self.system_ram_used_bytes

        system_ram_total_bytes = self.system_ram_total_bytes

        vram_used_bytes = self.vram_used_bytes

        vram_total_bytes = self.vram_total_bytes

        gpus = []
        for gpus_item_data in self.gpus:
            gpus_item = gpus_item_data.to_dict()
            gpus.append(gpus_item)

        slots = []
        for slots_item_data in self.slots:
            slots_item = slots_item_data.to_dict()
            slots.append(slots_item)

        spec_acceptance_rate: float | None | Unset
        if isinstance(self.spec_acceptance_rate, Unset):
            spec_acceptance_rate = UNSET
        else:
            spec_acceptance_rate = self.spec_acceptance_rate

        ram_breakdown: dict[str, Any] | None | Unset
        if isinstance(self.ram_breakdown, Unset):
            ram_breakdown = UNSET
        elif isinstance(self.ram_breakdown, MemoryBreakdown):
            ram_breakdown = self.ram_breakdown.to_dict()
        else:
            ram_breakdown = self.ram_breakdown

        vram_breakdown: dict[str, Any] | None | Unset
        if isinstance(self.vram_breakdown, Unset):
            vram_breakdown = UNSET
        elif isinstance(self.vram_breakdown, MemoryBreakdown):
            vram_breakdown = self.vram_breakdown.to_dict()
        else:
            vram_breakdown = self.vram_breakdown

        context_size: int | None | Unset
        if isinstance(self.context_size, Unset):
            context_size = UNSET
        else:
            context_size = self.context_size

        context_used_tokens: int | None | Unset
        if isinstance(self.context_used_tokens, Unset):
            context_used_tokens = UNSET
        else:
            context_used_tokens = self.context_used_tokens

        totals: dict[str, Any] | None | Unset
        if isinstance(self.totals, Unset):
            totals = UNSET
        elif isinstance(self.totals, ServerTotals):
            totals = self.totals.to_dict()
        else:
            totals = self.totals

        last_request: dict[str, Any] | None | Unset
        if isinstance(self.last_request, Unset):
            last_request = UNSET
        elif isinstance(self.last_request, LastRequest):
            last_request = self.last_request.to_dict()
        else:
            last_request = self.last_request

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_online": server_online,
                "predicted_tokens_per_sec": predicted_tokens_per_sec,
                "prompt_tokens_per_sec": prompt_tokens_per_sec,
                "avg_predicted_tokens_per_sec": avg_predicted_tokens_per_sec,
                "avg_prompt_tokens_per_sec": avg_prompt_tokens_per_sec,
                "kv_cache_usage_ratio": kv_cache_usage_ratio,
                "kv_cache_available": kv_cache_available,
                "active_slots": active_slots,
                "total_slots": total_slots,
                "requests_processing": requests_processing,
                "requests_deferred": requests_deferred,
                "system_ram_used_bytes": system_ram_used_bytes,
                "system_ram_total_bytes": system_ram_total_bytes,
                "vram_used_bytes": vram_used_bytes,
                "vram_total_bytes": vram_total_bytes,
                "gpus": gpus,
                "slots": slots,
            }
        )
        if spec_acceptance_rate is not UNSET:
            field_dict["spec_acceptance_rate"] = spec_acceptance_rate
        if ram_breakdown is not UNSET:
            field_dict["ram_breakdown"] = ram_breakdown
        if vram_breakdown is not UNSET:
            field_dict["vram_breakdown"] = vram_breakdown
        if context_size is not UNSET:
            field_dict["context_size"] = context_size
        if context_used_tokens is not UNSET:
            field_dict["context_used_tokens"] = context_used_tokens
        if totals is not UNSET:
            field_dict["totals"] = totals
        if last_request is not UNSET:
            field_dict["last_request"] = last_request

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gpu_memory import GpuMemory  # noqa: PLC0415
        from ..models.last_request import LastRequest  # noqa: PLC0415
        from ..models.memory_breakdown import MemoryBreakdown  # noqa: PLC0415
        from ..models.server_totals import ServerTotals  # noqa: PLC0415
        from ..models.slot_info import SlotInfo  # noqa: PLC0415

        d = dict(src_dict)
        server_online = d.pop("server_online")

        predicted_tokens_per_sec = d.pop("predicted_tokens_per_sec")

        prompt_tokens_per_sec = d.pop("prompt_tokens_per_sec")

        avg_predicted_tokens_per_sec = d.pop("avg_predicted_tokens_per_sec")

        avg_prompt_tokens_per_sec = d.pop("avg_prompt_tokens_per_sec")

        kv_cache_usage_ratio = d.pop("kv_cache_usage_ratio")

        kv_cache_available = d.pop("kv_cache_available")

        active_slots = d.pop("active_slots")

        total_slots = d.pop("total_slots")

        requests_processing = d.pop("requests_processing")

        requests_deferred = d.pop("requests_deferred")

        system_ram_used_bytes = d.pop("system_ram_used_bytes")

        system_ram_total_bytes = d.pop("system_ram_total_bytes")

        vram_used_bytes = d.pop("vram_used_bytes")

        vram_total_bytes = d.pop("vram_total_bytes")

        gpus = []
        _gpus = d.pop("gpus")
        for gpus_item_data in _gpus:
            gpus_item = GpuMemory.from_dict(gpus_item_data)

            gpus.append(gpus_item)

        slots = []
        _slots = d.pop("slots")
        for slots_item_data in _slots:
            slots_item = SlotInfo.from_dict(slots_item_data)

            slots.append(slots_item)

        def _parse_spec_acceptance_rate(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        spec_acceptance_rate = _parse_spec_acceptance_rate(d.pop("spec_acceptance_rate", UNSET))

        def _parse_ram_breakdown(data: object) -> MemoryBreakdown | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                ram_breakdown_type_1 = MemoryBreakdown.from_dict(data)

                return ram_breakdown_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MemoryBreakdown | None | Unset, data)

        ram_breakdown = _parse_ram_breakdown(d.pop("ram_breakdown", UNSET))

        def _parse_vram_breakdown(data: object) -> MemoryBreakdown | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                vram_breakdown_type_1 = MemoryBreakdown.from_dict(data)

                return vram_breakdown_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MemoryBreakdown | None | Unset, data)

        vram_breakdown = _parse_vram_breakdown(d.pop("vram_breakdown", UNSET))

        def _parse_context_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        context_size = _parse_context_size(d.pop("context_size", UNSET))

        def _parse_context_used_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        context_used_tokens = _parse_context_used_tokens(d.pop("context_used_tokens", UNSET))

        def _parse_totals(data: object) -> None | ServerTotals | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                totals_type_1 = ServerTotals.from_dict(data)

                return totals_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ServerTotals | Unset, data)

        totals = _parse_totals(d.pop("totals", UNSET))

        def _parse_last_request(data: object) -> LastRequest | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                last_request_type_1 = LastRequest.from_dict(data)

                return last_request_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LastRequest | None | Unset, data)

        last_request = _parse_last_request(d.pop("last_request", UNSET))

        telemetry_snapshot = cls(
            server_online=server_online,
            predicted_tokens_per_sec=predicted_tokens_per_sec,
            prompt_tokens_per_sec=prompt_tokens_per_sec,
            avg_predicted_tokens_per_sec=avg_predicted_tokens_per_sec,
            avg_prompt_tokens_per_sec=avg_prompt_tokens_per_sec,
            kv_cache_usage_ratio=kv_cache_usage_ratio,
            kv_cache_available=kv_cache_available,
            active_slots=active_slots,
            total_slots=total_slots,
            requests_processing=requests_processing,
            requests_deferred=requests_deferred,
            system_ram_used_bytes=system_ram_used_bytes,
            system_ram_total_bytes=system_ram_total_bytes,
            vram_used_bytes=vram_used_bytes,
            vram_total_bytes=vram_total_bytes,
            gpus=gpus,
            slots=slots,
            spec_acceptance_rate=spec_acceptance_rate,
            ram_breakdown=ram_breakdown,
            vram_breakdown=vram_breakdown,
            context_size=context_size,
            context_used_tokens=context_used_tokens,
            totals=totals,
            last_request=last_request,
        )

        telemetry_snapshot.additional_properties = d
        return telemetry_snapshot

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
