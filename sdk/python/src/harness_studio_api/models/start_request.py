from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.start_request_overrides import StartRequestOverrides


T = TypeVar("T", bound="StartRequest")


@_attrs_define
class StartRequest:
    """
    Attributes:
        definition_id (None | str | Unset): Definition to serve (instances without a profile). A profile serves its own.
        overrides (StartRequestOverrides | Unset): Catalog parameters layered over the definition/profile for this start
            only.
    """

    definition_id: None | str | Unset = UNSET
    overrides: StartRequestOverrides | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        definition_id: None | str | Unset
        if isinstance(self.definition_id, Unset):
            definition_id = UNSET
        else:
            definition_id = self.definition_id

        overrides: dict[str, Any] | Unset = UNSET
        if not isinstance(self.overrides, Unset):
            overrides = self.overrides.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if definition_id is not UNSET:
            field_dict["definition_id"] = definition_id
        if overrides is not UNSET:
            field_dict["overrides"] = overrides

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.start_request_overrides import (
            StartRequestOverrides,  # noqa: PLC0415
        )

        d = dict(src_dict)

        def _parse_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        definition_id = _parse_definition_id(d.pop("definition_id", UNSET))

        _overrides = d.pop("overrides", UNSET)
        overrides: StartRequestOverrides | Unset
        if isinstance(_overrides, Unset):
            overrides = UNSET
        else:
            overrides = StartRequestOverrides.from_dict(_overrides)

        start_request = cls(
            definition_id=definition_id,
            overrides=overrides,
        )

        start_request.additional_properties = d
        return start_request

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
