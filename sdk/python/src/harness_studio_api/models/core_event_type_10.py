from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_10_type import (
    CoreEventType10Type,
    check_core_event_type_10_type,
)

if TYPE_CHECKING:
    from ..models.restart_state import RestartState


T = TypeVar("T", bound="CoreEventType10")


@_attrs_define
class CoreEventType10:
    """An automatic restart after a crash was scheduled (attempt k of N, in `delay_secs`).

    Attributes:
        server_id (str):
        restart (RestartState): A scheduled automatic restart (shown as "restarting in N s, attempt k/N").
        type_ (CoreEventType10Type):
    """

    server_id: str
    restart: RestartState
    type_: CoreEventType10Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        restart = self.restart.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_id": server_id,
                "restart": restart,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.restart_state import RestartState  # noqa: PLC0415

        d = dict(src_dict)
        server_id = d.pop("server_id")

        restart = RestartState.from_dict(d.pop("restart"))

        type_ = check_core_event_type_10_type(d.pop("type"))

        core_event_type_10 = cls(
            server_id=server_id,
            restart=restart,
            type_=type_,
        )

        core_event_type_10.additional_properties = d
        return core_event_type_10

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
