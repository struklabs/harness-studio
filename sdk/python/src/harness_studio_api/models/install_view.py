from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="InstallView")


@_attrs_define
class InstallView:
    """A registered Lemonade build.

    Attributes:
        id (str):
        name (str):
        added_at (int):
        active (bool): The build the next start runs.
        version (None | str | Unset):
        tag (None | str | Unset): Release tag it was installed from.
    """

    id: str
    name: str
    added_at: int
    active: bool
    version: None | str | Unset = UNSET
    tag: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        added_at = self.added_at

        active = self.active

        version: None | str | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        else:
            version = self.version

        tag: None | str | Unset
        if isinstance(self.tag, Unset):
            tag = UNSET
        else:
            tag = self.tag

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "added_at": added_at,
                "active": active,
            }
        )
        if version is not UNSET:
            field_dict["version"] = version
        if tag is not UNSET:
            field_dict["tag"] = tag

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        added_at = d.pop("added_at")

        active = d.pop("active")

        def _parse_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        version = _parse_version(d.pop("version", UNSET))

        def _parse_tag(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tag = _parse_tag(d.pop("tag", UNSET))

        install_view = cls(
            id=id,
            name=name,
            added_at=added_at,
            active=active,
            version=version,
            tag=tag,
        )

        install_view.additional_properties = d
        return install_view

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
