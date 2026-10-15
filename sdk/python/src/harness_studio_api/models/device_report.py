from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.runtime_device import RuntimeDevice
    from ..models.split_suggestion import SplitSuggestion


T = TypeVar("T", bound="DeviceReport")


@_attrs_define
class DeviceReport:
    """
    Attributes:
        devices (list[RuntimeDevice]):
        probed_at (int): Unix seconds of the `--list-devices` run.
        binary (None | str | Unset):
        error (None | str | Unset): The probe failed (`devices` then come from the capability cache, if any).
        split (None | SplitSuggestion | Unset):
    """

    devices: list[RuntimeDevice]
    probed_at: int
    binary: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    split: None | SplitSuggestion | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.split_suggestion import SplitSuggestion  # noqa: PLC0415

        devices = []
        for devices_item_data in self.devices:
            devices_item = devices_item_data.to_dict()
            devices.append(devices_item)

        probed_at = self.probed_at

        binary: None | str | Unset
        if isinstance(self.binary, Unset):
            binary = UNSET
        else:
            binary = self.binary

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        split: dict[str, Any] | None | Unset
        if isinstance(self.split, Unset):
            split = UNSET
        elif isinstance(self.split, SplitSuggestion):
            split = self.split.to_dict()
        else:
            split = self.split

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "devices": devices,
                "probed_at": probed_at,
            }
        )
        if binary is not UNSET:
            field_dict["binary"] = binary
        if error is not UNSET:
            field_dict["error"] = error
        if split is not UNSET:
            field_dict["split"] = split

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.runtime_device import RuntimeDevice  # noqa: PLC0415
        from ..models.split_suggestion import SplitSuggestion  # noqa: PLC0415

        d = dict(src_dict)
        devices = []
        _devices = d.pop("devices")
        for devices_item_data in _devices:
            devices_item = RuntimeDevice.from_dict(devices_item_data)

            devices.append(devices_item)

        probed_at = d.pop("probed_at")

        def _parse_binary(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        binary = _parse_binary(d.pop("binary", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_split(data: object) -> None | SplitSuggestion | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                split_type_1 = SplitSuggestion.from_dict(data)

                return split_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SplitSuggestion | Unset, data)

        split = _parse_split(d.pop("split", UNSET))

        device_report = cls(
            devices=devices,
            probed_at=probed_at,
            binary=binary,
            error=error,
            split=split,
        )

        device_report.additional_properties = d
        return device_report

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
