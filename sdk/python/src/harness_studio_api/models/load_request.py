from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="LoadRequest")


@_attrs_define
class LoadRequest:
    """
    Attributes:
        definition_id (None | str | Unset): Definition id, or use `alias`.
        alias (None | str | Unset): Definition alias (the served model name).
        server_id (None | str | Unset): Instance or profile to load it on; omitted = a new instance on the next free
            port.
    """

    definition_id: None | str | Unset = UNSET
    alias: None | str | Unset = UNSET
    server_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        definition_id: None | str | Unset
        if isinstance(self.definition_id, Unset):
            definition_id = UNSET
        else:
            definition_id = self.definition_id

        alias: None | str | Unset
        if isinstance(self.alias, Unset):
            alias = UNSET
        else:
            alias = self.alias

        server_id: None | str | Unset
        if isinstance(self.server_id, Unset):
            server_id = UNSET
        else:
            server_id = self.server_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if definition_id is not UNSET:
            field_dict["definition_id"] = definition_id
        if alias is not UNSET:
            field_dict["alias"] = alias
        if server_id is not UNSET:
            field_dict["server_id"] = server_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        definition_id = _parse_definition_id(d.pop("definition_id", UNSET))

        def _parse_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        alias = _parse_alias(d.pop("alias", UNSET))

        def _parse_server_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        server_id = _parse_server_id(d.pop("server_id", UNSET))

        load_request = cls(
            definition_id=definition_id,
            alias=alias,
            server_id=server_id,
        )

        load_request.additional_properties = d
        return load_request

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
