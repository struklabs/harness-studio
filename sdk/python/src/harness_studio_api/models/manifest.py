from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="Manifest")


@_attrs_define
class Manifest:
    """`manifest.json` of the environment.

    Attributes:
        uv_version (str):
        python (str): `Python 3.12.13`.
        tag (str): llama.cpp tag of the converter source the packages were installed for.
        requirements_sha256 (str): SHA-256 over the requirement files of that tag (a different set means a rebuild).
        installed_at (int):
    """

    uv_version: str
    python: str
    tag: str
    requirements_sha256: str
    installed_at: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        uv_version = self.uv_version

        python = self.python

        tag = self.tag

        requirements_sha256 = self.requirements_sha256

        installed_at = self.installed_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "uv_version": uv_version,
                "python": python,
                "tag": tag,
                "requirements_sha256": requirements_sha256,
                "installed_at": installed_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        uv_version = d.pop("uv_version")

        python = d.pop("python")

        tag = d.pop("tag")

        requirements_sha256 = d.pop("requirements_sha256")

        installed_at = d.pop("installed_at")

        manifest = cls(
            uv_version=uv_version,
            python=python,
            tag=tag,
            requirements_sha256=requirements_sha256,
            installed_at=installed_at,
        )

        manifest.additional_properties = d
        return manifest

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
