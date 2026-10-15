from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ManagedInstall")


@_attrs_define
class ManagedInstall:
    """Result of [`install_managed`].

    Attributes:
        version (str): Release tag that was installed.
        assets (list[str]): Archives that were downloaded.
        version_text (str): `llama-server --version` of the new build.
        backend (None | str | Unset): Backend of the installed build.
        previous_version (None | str | Unset): The build it replaced (now in the backup slot).
    """

    version: str
    assets: list[str]
    version_text: str
    backend: None | str | Unset = UNSET
    previous_version: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        version = self.version

        assets = self.assets

        version_text = self.version_text

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        previous_version: None | str | Unset
        if isinstance(self.previous_version, Unset):
            previous_version = UNSET
        else:
            previous_version = self.previous_version

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "version": version,
                "assets": assets,
                "version_text": version_text,
            }
        )
        if backend is not UNSET:
            field_dict["backend"] = backend
        if previous_version is not UNSET:
            field_dict["previous_version"] = previous_version

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        version = d.pop("version")

        assets = cast(list[str], d.pop("assets"))

        version_text = d.pop("version_text")

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        def _parse_previous_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        previous_version = _parse_previous_version(d.pop("previous_version", UNSET))

        managed_install = cls(
            version=version,
            assets=assets,
            version_text=version_text,
            backend=backend,
            previous_version=previous_version,
        )

        managed_install.additional_properties = d
        return managed_install

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
