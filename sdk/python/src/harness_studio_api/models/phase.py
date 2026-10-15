from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="Phase")


@_attrs_define
class Phase:
    """One segment of a request's timeline, in ms from its arrival at the gateway.

    Attributes:
        kind (str): `load` (JIT load of the model), `queue` (waiting for a free slot), `prompt` (prompt
            processing), `generation` (token generation), `other` (proxy, network, sampling
            overhead after the model's own timings).
        start_ms (float):
        duration_ms (float):
    """

    kind: str
    start_ms: float
    duration_ms: float
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        kind = self.kind

        start_ms = self.start_ms

        duration_ms = self.duration_ms

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "kind": kind,
                "start_ms": start_ms,
                "duration_ms": duration_ms,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        kind = d.pop("kind")

        start_ms = d.pop("start_ms")

        duration_ms = d.pop("duration_ms")

        phase = cls(
            kind=kind,
            start_ms=start_ms,
            duration_ms=duration_ms,
        )

        phase.additional_properties = d
        return phase

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
