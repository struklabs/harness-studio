from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.core_event_type_2_type import (
    CoreEventType2Type,
    check_core_event_type_2_type,
)
from ..models.server_status_type_0 import ServerStatusType0, check_server_status_type_0
from ..models.server_status_type_1 import ServerStatusType1, check_server_status_type_1
from ..models.server_status_type_2 import ServerStatusType2, check_server_status_type_2
from ..models.server_status_type_3 import ServerStatusType3, check_server_status_type_3

if TYPE_CHECKING:
    from ..models.server_status_type_4 import ServerStatusType4


T = TypeVar("T", bound="CoreEventType2")


@_attrs_define
class CoreEventType2:
    """A server instance changed status (starting, running, stopped, error...).

    Attributes:
        server_id (str):
        status (ServerStatusType0 | ServerStatusType1 | ServerStatusType2 | ServerStatusType3 | ServerStatusType4):
        type_ (CoreEventType2Type):
    """

    server_id: str
    status: (
        ServerStatusType0
        | ServerStatusType1
        | ServerStatusType2
        | ServerStatusType3
        | ServerStatusType4
    )
    type_: CoreEventType2Type
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        status: dict[str, Any] | str
        if isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        else:
            status = self.status.to_dict()

        type_: str = self.type_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_id": server_id,
                "status": status,
                "type": type_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.server_status_type_4 import ServerStatusType4  # noqa: PLC0415

        d = dict(src_dict)
        server_id = d.pop("server_id")

        def _parse_status(
            data: object,
        ) -> (
            ServerStatusType0
            | ServerStatusType1
            | ServerStatusType2
            | ServerStatusType3
            | ServerStatusType4
        ):
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_0 = check_server_status_type_0(data)

                return componentsschemas_server_status_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_1 = check_server_status_type_1(data)

                return componentsschemas_server_status_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_2 = check_server_status_type_2(data)

                return componentsschemas_server_status_type_2
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_3 = check_server_status_type_3(data)

                return componentsschemas_server_status_type_3
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_server_status_type_4 = ServerStatusType4.from_dict(data)

            return componentsschemas_server_status_type_4

        status = _parse_status(d.pop("status"))

        type_ = check_core_event_type_2_type(d.pop("type"))

        core_event_type_2 = cls(
            server_id=server_id,
            status=status,
            type_=type_,
        )

        core_event_type_2.additional_properties = d
        return core_event_type_2

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
