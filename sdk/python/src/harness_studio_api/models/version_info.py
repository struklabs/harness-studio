from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="VersionInfo")


@_attrs_define
class VersionInfo:
    """
    Attributes:
        raw (str): First line of the `--version` output, verbatim.
        version (None | str | Unset): `0.5.0-dev` on current builds; the build number on old ones (`version: 4589
            (hash)`).
        build (int | None | Unset):
        commit (None | str | Unset):
        built_with (None | str | Unset): `built with Clang 20.1.8 for Windows x86_64`
    """

    raw: str
    version: None | str | Unset = UNSET
    build: int | None | Unset = UNSET
    commit: None | str | Unset = UNSET
    built_with: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        raw = self.raw

        version: None | str | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        else:
            version = self.version

        build: int | None | Unset
        if isinstance(self.build, Unset):
            build = UNSET
        else:
            build = self.build

        commit: None | str | Unset
        if isinstance(self.commit, Unset):
            commit = UNSET
        else:
            commit = self.commit

        built_with: None | str | Unset
        if isinstance(self.built_with, Unset):
            built_with = UNSET
        else:
            built_with = self.built_with

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "raw": raw,
            }
        )
        if version is not UNSET:
            field_dict["version"] = version
        if build is not UNSET:
            field_dict["build"] = build
        if commit is not UNSET:
            field_dict["commit"] = commit
        if built_with is not UNSET:
            field_dict["built_with"] = built_with

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        raw = d.pop("raw")

        def _parse_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        version = _parse_version(d.pop("version", UNSET))

        def _parse_build(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        build = _parse_build(d.pop("build", UNSET))

        def _parse_commit(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        commit = _parse_commit(d.pop("commit", UNSET))

        def _parse_built_with(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        built_with = _parse_built_with(d.pop("built_with", UNSET))

        version_info = cls(
            raw=raw,
            version=version,
            build=build,
            commit=commit,
            built_with=built_with,
        )

        version_info.additional_properties = d
        return version_info

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
