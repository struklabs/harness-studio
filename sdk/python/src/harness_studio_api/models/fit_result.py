from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.estimate_source import EstimateSource, check_estimate_source
from ..models.fit_category import FitCategory, check_fit_category
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.kv_choice import KvChoice
    from ..models.memory_budget import MemoryBudget
    from ..models.memory_estimate import MemoryEstimate
    from ..models.slot_advice import SlotAdvice


T = TypeVar("T", bound="FitResult")


@_attrs_define
class FitResult:
    """
    Attributes:
        category (FitCategory):
        estimate (MemoryEstimate): Where the memory goes, in bytes.
        budget (MemoryBudget): Memory usable for the model.
        vram_percentage (int):
        ram_percentage (int):
        source (EstimateSource):
        recommended_gpu_layers (int): The `-ngl` that keeps the estimate under 90% of the budget (layer count + 1 =
            full).
        measured_at (int | None | Unset): Unix ms of the measurement used (`source` corrected/measured).
        slots (None | SlotAdvice | Unset):
        kv (KvChoice | None | Unset):
    """

    category: FitCategory
    estimate: MemoryEstimate
    budget: MemoryBudget
    vram_percentage: int
    ram_percentage: int
    source: EstimateSource
    recommended_gpu_layers: int
    measured_at: int | None | Unset = UNSET
    slots: None | SlotAdvice | Unset = UNSET
    kv: KvChoice | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.kv_choice import KvChoice  # noqa: PLC0415
        from ..models.slot_advice import SlotAdvice  # noqa: PLC0415

        category: str = self.category

        estimate = self.estimate.to_dict()

        budget = self.budget.to_dict()

        vram_percentage = self.vram_percentage

        ram_percentage = self.ram_percentage

        source: str = self.source

        recommended_gpu_layers = self.recommended_gpu_layers

        measured_at: int | None | Unset
        if isinstance(self.measured_at, Unset):
            measured_at = UNSET
        else:
            measured_at = self.measured_at

        slots: dict[str, Any] | None | Unset
        if isinstance(self.slots, Unset):
            slots = UNSET
        elif isinstance(self.slots, SlotAdvice):
            slots = self.slots.to_dict()
        else:
            slots = self.slots

        kv: dict[str, Any] | None | Unset
        if isinstance(self.kv, Unset):
            kv = UNSET
        elif isinstance(self.kv, KvChoice):
            kv = self.kv.to_dict()
        else:
            kv = self.kv

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "category": category,
                "estimate": estimate,
                "budget": budget,
                "vram_percentage": vram_percentage,
                "ram_percentage": ram_percentage,
                "source": source,
                "recommended_gpu_layers": recommended_gpu_layers,
            }
        )
        if measured_at is not UNSET:
            field_dict["measured_at"] = measured_at
        if slots is not UNSET:
            field_dict["slots"] = slots
        if kv is not UNSET:
            field_dict["kv"] = kv

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.kv_choice import KvChoice  # noqa: PLC0415
        from ..models.memory_budget import MemoryBudget  # noqa: PLC0415
        from ..models.memory_estimate import MemoryEstimate  # noqa: PLC0415
        from ..models.slot_advice import SlotAdvice  # noqa: PLC0415

        d = dict(src_dict)
        category = check_fit_category(d.pop("category"))

        estimate = MemoryEstimate.from_dict(d.pop("estimate"))

        budget = MemoryBudget.from_dict(d.pop("budget"))

        vram_percentage = d.pop("vram_percentage")

        ram_percentage = d.pop("ram_percentage")

        source = check_estimate_source(d.pop("source"))

        recommended_gpu_layers = d.pop("recommended_gpu_layers")

        def _parse_measured_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        measured_at = _parse_measured_at(d.pop("measured_at", UNSET))

        def _parse_slots(data: object) -> None | SlotAdvice | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                slots_type_1 = SlotAdvice.from_dict(data)

                return slots_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SlotAdvice | Unset, data)

        slots = _parse_slots(d.pop("slots", UNSET))

        def _parse_kv(data: object) -> KvChoice | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                kv_type_1 = KvChoice.from_dict(data)

                return kv_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(KvChoice | None | Unset, data)

        kv = _parse_kv(d.pop("kv", UNSET))

        fit_result = cls(
            category=category,
            estimate=estimate,
            budget=budget,
            vram_percentage=vram_percentage,
            ram_percentage=ram_percentage,
            source=source,
            recommended_gpu_layers=recommended_gpu_layers,
            measured_at=measured_at,
            slots=slots,
            kv=kv,
        )

        fit_result.additional_properties = d
        return fit_result

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
