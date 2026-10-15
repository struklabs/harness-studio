from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.retry_request_overrides import RetryRequestOverrides


T = TypeVar("T", bound="RetryRequest")


@_attrs_define
class RetryRequest:
    """
    Attributes:
        option (None | str | Unset): A recovery option id from `GET /servers/{id}/recovery` (its overrides and servers
            to
            stop are used).
        overrides (RetryRequestOverrides | Unset): Catalog parameters over the last launch (added to the option's).
        unload (list[str] | Unset): Servers to stop first (added to the option's).
        save (bool | Unset): Also save the values to the model definition (and the profile's own overrides).
    """

    option: None | str | Unset = UNSET
    overrides: RetryRequestOverrides | Unset = UNSET
    unload: list[str] | Unset = UNSET
    save: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        option: None | str | Unset
        if isinstance(self.option, Unset):
            option = UNSET
        else:
            option = self.option

        overrides: dict[str, Any] | Unset = UNSET
        if not isinstance(self.overrides, Unset):
            overrides = self.overrides.to_dict()

        unload: list[str] | Unset = UNSET
        if not isinstance(self.unload, Unset):
            unload = self.unload

        save = self.save

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if option is not UNSET:
            field_dict["option"] = option
        if overrides is not UNSET:
            field_dict["overrides"] = overrides
        if unload is not UNSET:
            field_dict["unload"] = unload
        if save is not UNSET:
            field_dict["save"] = save

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.retry_request_overrides import (
            RetryRequestOverrides,  # noqa: PLC0415
        )

        d = dict(src_dict)

        def _parse_option(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        option = _parse_option(d.pop("option", UNSET))

        _overrides = d.pop("overrides", UNSET)
        overrides: RetryRequestOverrides | Unset
        if isinstance(_overrides, Unset):
            overrides = UNSET
        else:
            overrides = RetryRequestOverrides.from_dict(_overrides)

        unload = cast(list[str], d.pop("unload", UNSET))

        save = d.pop("save", UNSET)

        retry_request = cls(
            option=option,
            overrides=overrides,
            unload=unload,
            save=save,
        )

        retry_request.additional_properties = d
        return retry_request

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
