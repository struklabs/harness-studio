from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="AddLocal")


@_attrs_define
class AddLocal:
    """A local folder to register.

    Attributes:
        path (str): Folder with `llama-server` (or the `llama-server` file itself).
        name (str):
        id (None | str | Unset): Wanted id (default: from the name).
        copy (bool | Unset): Copy the folder's top-level files under the data dir instead of using it in place.
        family (str | Unset): Runtime family of the folder (default llama.cpp).
    """

    path: str
    name: str
    id: None | str | Unset = UNSET
    copy: bool | Unset = UNSET
    family: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        path = self.path

        name = self.name

        id: None | str | Unset
        if isinstance(self.id, Unset):
            id = UNSET
        else:
            id = self.id

        copy = self.copy

        family = self.family

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "path": path,
                "name": name,
            }
        )
        if id is not UNSET:
            field_dict["id"] = id
        if copy is not UNSET:
            field_dict["copy"] = copy
        if family is not UNSET:
            field_dict["family"] = family

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        path = d.pop("path")

        name = d.pop("name")

        def _parse_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        id = _parse_id(d.pop("id", UNSET))

        copy = d.pop("copy", UNSET)

        family = d.pop("family", UNSET)

        add_local = cls(
            path=path,
            name=name,
            id=id,
            copy=copy,
            family=family,
        )

        add_local.additional_properties = d
        return add_local

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
