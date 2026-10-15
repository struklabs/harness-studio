from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="SlotAdvice")


@_attrs_define
class SlotAdvice:
    """How many concurrent requests a server serves, and why.

    Attributes:
        slots (int): Slots the server runs with.
        auto (bool): `true` = chosen by the automatic rule (the setting is "Auto").
        recommended (int): What the automatic rule picks for this model, context and machine.
        recommended_agents (int): What it picks when the model serves agents directly (a server prepared for an agent
            tool; the gateway's own loads use the smaller request size, see [`SlotOrigin::Gateway`]).
        reason (str): i18n key of the one-line reason of `recommended` (`slots.reason.*`).
        per_slot_bytes (int): Memory one more slot adds (sliding-window / recurrent state on the device +
            checkpoints in host memory).
        ctx_per_slot (int): Context each of the `recommended` concurrent requests can count on.
    """

    slots: int
    auto: bool
    recommended: int
    recommended_agents: int
    reason: str
    per_slot_bytes: int
    ctx_per_slot: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        slots = self.slots

        auto = self.auto

        recommended = self.recommended

        recommended_agents = self.recommended_agents

        reason = self.reason

        per_slot_bytes = self.per_slot_bytes

        ctx_per_slot = self.ctx_per_slot

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "slots": slots,
                "auto": auto,
                "recommended": recommended,
                "recommended_agents": recommended_agents,
                "reason": reason,
                "per_slot_bytes": per_slot_bytes,
                "ctx_per_slot": ctx_per_slot,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        slots = d.pop("slots")

        auto = d.pop("auto")

        recommended = d.pop("recommended")

        recommended_agents = d.pop("recommended_agents")

        reason = d.pop("reason")

        per_slot_bytes = d.pop("per_slot_bytes")

        ctx_per_slot = d.pop("ctx_per_slot")

        slot_advice = cls(
            slots=slots,
            auto=auto,
            recommended=recommended,
            recommended_agents=recommended_agents,
            reason=reason,
            per_slot_bytes=per_slot_bytes,
            ctx_per_slot=ctx_per_slot,
        )

        slot_advice.additional_properties = d
        return slot_advice

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
