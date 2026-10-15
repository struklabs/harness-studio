from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_9_type import (
    CoreEventType9Type,
    check_core_event_type_9_type,
)

if TYPE_CHECKING:
    from ..models.server_failure import ServerFailure


T = TypeVar("T", bound="CoreEventType9")


@_attrs_define
class CoreEventType9:
    """A server run failed; the classified cause (`servers.out_of_memory`...).

    Attributes:
        server_id (str):
        failure (ServerFailure): A classified failure of one run.
        type_ (CoreEventType9Type):
    """

    server_id: str
    failure: ServerFailure
    type_: CoreEventType9Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        failure = self.failure.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_id": server_id,
                "failure": failure,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.server_failure import ServerFailure  # noqa: PLC0415

        d = dict(src_dict)
        server_id = d.pop("server_id")

        failure = ServerFailure.from_dict(d.pop("failure"))

        type_ = check_core_event_type_9_type(d.pop("type"))

        core_event_type_9 = cls(
            server_id=server_id,
            failure=failure,
            type_=type_,
        )

        core_event_type_9.additional_properties = d
        return core_event_type_9

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
