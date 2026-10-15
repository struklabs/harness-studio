from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.binary_source import BinarySource, check_binary_source
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.device import Device
    from ..models.version_info import VersionInfo


T = TypeVar("T", bound="RuntimeSummary")


@_attrs_define
class RuntimeSummary:
    """The llama-server binary the next start uses and what it can do.

    Attributes:
        mode (str): Configured mode: `managed` (downloaded by Harness) or `custom`.
        fallback (bool): The configured mode can't be used, `source` is a fallback.
        help_parsed (bool): `--help` could be parsed.
        flag_count (int):
        devices (list[Device]):
        endpoint_verified (list[str]): Features verified against a running server of this binary.
        source (BinarySource | None | Unset):
        binary_path (None | str | Unset): `None` when no llama-server is installed/configured (`error` says why).
        error (None | str | Unset):
        version (None | Unset | VersionInfo):
        probed_at (int | None | Unset): Unix seconds of the probe.
    """

    mode: str
    fallback: bool
    help_parsed: bool
    flag_count: int
    devices: list[Device]
    endpoint_verified: list[str]
    source: BinarySource | None | Unset = UNSET
    binary_path: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    version: None | Unset | VersionInfo = UNSET
    probed_at: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.version_info import VersionInfo  # noqa: PLC0415

        mode = self.mode

        fallback = self.fallback

        help_parsed = self.help_parsed

        flag_count = self.flag_count

        devices = []
        for devices_item_data in self.devices:
            devices_item = devices_item_data.to_dict()
            devices.append(devices_item)

        endpoint_verified = self.endpoint_verified

        source: None | str | Unset
        if isinstance(self.source, Unset):
            source = UNSET
        elif isinstance(self.source, str):
            source = self.source
        else:
            source = self.source

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

        probed_at: int | None | Unset
        if isinstance(self.probed_at, Unset):
            probed_at = UNSET
        else:
            probed_at = self.probed_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "mode": mode,
                "fallback": fallback,
                "help_parsed": help_parsed,
                "flag_count": flag_count,
                "devices": devices,
                "endpoint_verified": endpoint_verified,
            }
        )
        if source is not UNSET:
            field_dict["source"] = source
        if binary_path is not UNSET:
            field_dict["binary_path"] = binary_path
        if error is not UNSET:
            field_dict["error"] = error
        if version is not UNSET:
            field_dict["version"] = version
        if probed_at is not UNSET:
            field_dict["probed_at"] = probed_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.device import Device  # noqa: PLC0415
        from ..models.version_info import VersionInfo  # noqa: PLC0415

        d = dict(src_dict)
        mode = d.pop("mode")

        fallback = d.pop("fallback")

        help_parsed = d.pop("help_parsed")

        flag_count = d.pop("flag_count")

        devices = []
        _devices = d.pop("devices")
        for devices_item_data in _devices:
            devices_item = Device.from_dict(devices_item_data)

            devices.append(devices_item)

        endpoint_verified = cast(list[str], d.pop("endpoint_verified"))

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

        def _parse_probed_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        probed_at = _parse_probed_at(d.pop("probed_at", UNSET))

        runtime_summary = cls(
            mode=mode,
            fallback=fallback,
            help_parsed=help_parsed,
            flag_count=flag_count,
            devices=devices,
            endpoint_verified=endpoint_verified,
            source=source,
            binary_path=binary_path,
            error=error,
            version=version,
            probed_at=probed_at,
        )

        runtime_summary.additional_properties = d
        return runtime_summary

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
