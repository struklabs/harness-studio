from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.official_state import OfficialState
    from ..models.runtime_entry import RuntimeEntry
    from ..models.version_info import VersionInfo


T = TypeVar("T", bound="RuntimeView")


@_attrs_define
class RuntimeView:
    """What the UI/API/CLI list shows for one runtime.

    Attributes:
        id (str):
        name (str): Empty for the official runtime (the UI shows its own label).
        kind (str): `official`, `github` or `local`.
        available (bool): The binary can be launched now (exists, files unchanged).
        help_parsed (bool): `--help` could be parsed (flag gating applies).
        flag_count (int):
        unsupported_params (list[str]): Catalog parameters this binary has no flag for (hidden and never sent for it).
        quant_types (list[str]):
        entry (None | RuntimeEntry | Unset):
        binary_path (None | str | Unset):
        error (None | str | Unset): Why not (error key + detail).
        version (None | Unset | VersionInfo):
        backend (None | str | Unset):
        official (None | OfficialState | Unset):
    """

    id: str
    name: str
    kind: str
    available: bool
    help_parsed: bool
    flag_count: int
    unsupported_params: list[str]
    quant_types: list[str]
    entry: None | RuntimeEntry | Unset = UNSET
    binary_path: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    version: None | Unset | VersionInfo = UNSET
    backend: None | str | Unset = UNSET
    official: None | OfficialState | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.official_state import OfficialState  # noqa: PLC0415
        from ..models.runtime_entry import RuntimeEntry  # noqa: PLC0415
        from ..models.version_info import VersionInfo  # noqa: PLC0415

        id = self.id

        name = self.name

        kind = self.kind

        available = self.available

        help_parsed = self.help_parsed

        flag_count = self.flag_count

        unsupported_params = self.unsupported_params

        quant_types = self.quant_types

        entry: dict[str, Any] | None | Unset
        if isinstance(self.entry, Unset):
            entry = UNSET
        elif isinstance(self.entry, RuntimeEntry):
            entry = self.entry.to_dict()
        else:
            entry = self.entry

        binary_path: None | str | Unset
        if isinstance(self.binary_path, Unset):
            binary_path = UNSET
        else:
            binary_path = self.binary_path

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        version: dict[str, Any] | None | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        elif isinstance(self.version, VersionInfo):
            version = self.version.to_dict()
        else:
            version = self.version

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        official: dict[str, Any] | None | Unset
        if isinstance(self.official, Unset):
            official = UNSET
        elif isinstance(self.official, OfficialState):
            official = self.official.to_dict()
        else:
            official = self.official

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "kind": kind,
                "available": available,
                "help_parsed": help_parsed,
                "flag_count": flag_count,
                "unsupported_params": unsupported_params,
                "quant_types": quant_types,
            }
        )
        if entry is not UNSET:
            field_dict["entry"] = entry
        if binary_path is not UNSET:
            field_dict["binary_path"] = binary_path
        if error is not UNSET:
            field_dict["error"] = error
        if version is not UNSET:
            field_dict["version"] = version
        if backend is not UNSET:
            field_dict["backend"] = backend
        if official is not UNSET:
            field_dict["official"] = official

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.official_state import OfficialState  # noqa: PLC0415
        from ..models.runtime_entry import RuntimeEntry  # noqa: PLC0415
        from ..models.version_info import VersionInfo  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        kind = d.pop("kind")

        available = d.pop("available")

        help_parsed = d.pop("help_parsed")

        flag_count = d.pop("flag_count")

        unsupported_params = cast(list[str], d.pop("unsupported_params"))

        quant_types = cast(list[str], d.pop("quant_types"))

        def _parse_entry(data: object) -> None | RuntimeEntry | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                entry_type_1 = RuntimeEntry.from_dict(data)

                return entry_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RuntimeEntry | Unset, data)

        entry = _parse_entry(d.pop("entry", UNSET))

        def _parse_binary_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        binary_path = _parse_binary_path(d.pop("binary_path", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_version(data: object) -> None | Unset | VersionInfo:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                version_type_1 = VersionInfo.from_dict(data)

                return version_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | VersionInfo, data)

        version = _parse_version(d.pop("version", UNSET))

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        def _parse_official(data: object) -> None | OfficialState | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                official_type_1 = OfficialState.from_dict(data)

                return official_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | OfficialState | Unset, data)

        official = _parse_official(d.pop("official", UNSET))

        runtime_view = cls(
            id=id,
            name=name,
            kind=kind,
            available=available,
            help_parsed=help_parsed,
            flag_count=flag_count,
            unsupported_params=unsupported_params,
            quant_types=quant_types,
            entry=entry,
            binary_path=binary_path,
            error=error,
            version=version,
            backend=backend,
            official=official,
        )

        runtime_view.additional_properties = d
        return runtime_view

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
