from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="QueueHint")


@_attrs_define
class QueueHint:
    """Queue behaviour of one definition over a time window and, when it keeps making requests
    wait for a free slot, how many slots to try at its next load (see
    [`AnalyticsDb::queue_hints`]).

        Attributes:
            definition_id (str):
            alias (str):
            slots (int): Slots of the server at its most recent request in the window; only requests served with
                that slot count are counted (an earlier load with other slots says nothing about it).
            requests (int):
            waited_slots (int): Requests that found every slot busy.
            waited_tokens (int): Requests that had a slot but waited for room in the shared KV pool (more slots would
                not help these).
            wait_share (float): `waited_slots / requests`.
            avg_wait_ms (float): Mean queue time of the slot-waiting requests, ms (loads excluded).
            suggested_slots (int | None | Unset): Slots worth trying at the next load; `None` while the queue is no problem.
    """

    definition_id: str
    alias: str
    slots: int
    requests: int
    waited_slots: int
    waited_tokens: int
    wait_share: float
    avg_wait_ms: float
    suggested_slots: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        definition_id = self.definition_id

        alias = self.alias

        slots = self.slots

        requests = self.requests

        waited_slots = self.waited_slots

        waited_tokens = self.waited_tokens

        wait_share = self.wait_share

        avg_wait_ms = self.avg_wait_ms

        suggested_slots: int | None | Unset
        if isinstance(self.suggested_slots, Unset):
            suggested_slots = UNSET
        else:
            suggested_slots = self.suggested_slots

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "definition_id": definition_id,
                "alias": alias,
                "slots": slots,
                "requests": requests,
                "waited_slots": waited_slots,
                "waited_tokens": waited_tokens,
                "wait_share": wait_share,
                "avg_wait_ms": avg_wait_ms,
            }
        )
        if suggested_slots is not UNSET:
            field_dict["suggested_slots"] = suggested_slots

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        definition_id = d.pop("definition_id")

        alias = d.pop("alias")

        slots = d.pop("slots")

        requests = d.pop("requests")

        waited_slots = d.pop("waited_slots")

        waited_tokens = d.pop("waited_tokens")

        wait_share = d.pop("wait_share")

        avg_wait_ms = d.pop("avg_wait_ms")

        def _parse_suggested_slots(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        suggested_slots = _parse_suggested_slots(d.pop("suggested_slots", UNSET))

        queue_hint = cls(
            definition_id=definition_id,
            alias=alias,
            slots=slots,
            requests=requests,
            waited_slots=waited_slots,
            waited_tokens=waited_tokens,
            wait_share=wait_share,
            avg_wait_ms=avg_wait_ms,
            suggested_slots=suggested_slots,
        )

        queue_hint.additional_properties = d
        return queue_hint

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
