from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.binary_source import BinarySource, check_binary_source
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.version_info import VersionInfo


T = TypeVar("T", bound="OfficialState")


@_attrs_define
class OfficialState:
    """The honest state of the official runtime (Settings > Runtime).

    Attributes:
        configured_mode (str): Configured mode: `managed` or `custom`.
        fallback (bool): The configured mode can't be honored (e.g. managed build not installed), so the binary
            used is a fallback (a system llama-server found on PATH / WinGet...).
        managed_installed (bool):
        source (BinarySource | None | Unset):
        managed_version (None | str | Unset): Version the settings record for the managed build.
        staged (None | Unset | VersionInfo):
    """

    configured_mode: str
    fallback: bool
    managed_installed: bool
    source: BinarySource | None | Unset = UNSET
    managed_version: None | str | Unset = UNSET
    staged: None | Unset | VersionInfo = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.version_info import VersionInfo  # noqa: PLC0415

        configured_mode = self.configured_mode

        fallback = self.fallback

        managed_installed = self.managed_installed

        source: None | str | Unset
        if isinstance(self.source, Unset):
            source = UNSET
        elif isinstance(self.source, str):
            source = self.source
        else:
            source = self.source

        managed_version: None | str | Unset
        if isinstance(self.managed_version, Unset):
            managed_version = UNSET
        else:
            managed_version = self.managed_version

        staged: dict[str, Any] | None | Unset
        if isinstance(self.staged, Unset):
            staged = UNSET
        elif isinstance(self.staged, VersionInfo):
            staged = self.staged.to_dict()
        else:
            staged = self.staged

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "configured_mode": configured_mode,
                "fallback": fallback,
                "managed_installed": managed_installed,
            }
        )
        if source is not UNSET:
            field_dict["source"] = source
        if managed_version is not UNSET:
            field_dict["managed_version"] = managed_version
        if staged is not UNSET:
            field_dict["staged"] = staged

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.version_info import VersionInfo  # noqa: PLC0415

        d = dict(src_dict)
        configured_mode = d.pop("configured_mode")

        fallback = d.pop("fallback")

        managed_installed = d.pop("managed_installed")

        def _parse_source(data: object) -> BinarySource | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                source_type_1 = check_binary_source(data)

                return source_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BinarySource | None | Unset, data)

        source = _parse_source(d.pop("source", UNSET))

        def _parse_managed_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        managed_version = _parse_managed_version(d.pop("managed_version", UNSET))

        def _parse_staged(data: object) -> None | Unset | VersionInfo:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                staged_type_1 = VersionInfo.from_dict(data)

                return staged_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | VersionInfo, data)

        staged = _parse_staged(d.pop("staged", UNSET))

        official_state = cls(
            configured_mode=configured_mode,
            fallback=fallback,
            managed_installed=managed_installed,
            source=source,
            managed_version=managed_version,
            staged=staged,
        )

        official_state.additional_properties = d
        return official_state

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
