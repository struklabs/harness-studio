from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="RestartState")


@_attrs_define
class RestartState:
    """A scheduled automatic restart (shown as "restarting in N s, attempt k/N").

    Attributes:
        attempt (int): 1-based attempt number of the scheduled restart.
        max_attempts (int):
        delay_secs (int):
        restart_at_ms (int): Unix milliseconds when the restart begins.
    """

    attempt: int
    max_attempts: int
    delay_secs: int
    restart_at_ms: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        attempt = self.attempt

        max_attempts = self.max_attempts

        delay_secs = self.delay_secs

        restart_at_ms = self.restart_at_ms

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "attempt": attempt,
                "max_attempts": max_attempts,
                "delay_secs": delay_secs,
                "restart_at_ms": restart_at_ms,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        attempt = d.pop("attempt")

        max_attempts = d.pop("max_attempts")

        delay_secs = d.pop("delay_secs")

        restart_at_ms = d.pop("restart_at_ms")

        restart_state = cls(
            attempt=attempt,
            max_attempts=max_attempts,
            delay_secs=delay_secs,
            restart_at_ms=restart_at_ms,
        )

        restart_state.additional_properties = d
        return restart_state

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
