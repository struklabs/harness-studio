from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.effective_launch_values import EffectiveLaunchValues
    from ..models.launch_config import LaunchConfig


T = TypeVar("T", bound="EffectiveResponse")


@_attrs_define
class EffectiveResponse:
    """
    Attributes:
        config (LaunchConfig | None | Unset):
        values (EffectiveLaunchValues | None | Unset):
    """

    config: LaunchConfig | None | Unset = UNSET
    values: EffectiveLaunchValues | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.effective_launch_values import (
            EffectiveLaunchValues,  # noqa: PLC0415
        )
        from ..models.launch_config import LaunchConfig  # noqa: PLC0415

        config: dict[str, Any] | None | Unset
        if isinstance(self.config, Unset):
            config = UNSET
        elif isinstance(self.config, LaunchConfig):
            config = self.config.to_dict()
        else:
            config = self.config

        values: dict[str, Any] | None | Unset
        if isinstance(self.values, Unset):
            values = UNSET
        elif isinstance(self.values, EffectiveLaunchValues):
            values = self.values.to_dict()
        else:
            values = self.values

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if config is not UNSET:
            field_dict["config"] = config
        if values is not UNSET:
            field_dict["values"] = values

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.effective_launch_values import (
            EffectiveLaunchValues,  # noqa: PLC0415
        )
        from ..models.launch_config import LaunchConfig  # noqa: PLC0415

        d = dict(src_dict)

        def _parse_config(data: object) -> LaunchConfig | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                config_type_1 = LaunchConfig.from_dict(data)

                return config_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LaunchConfig | None | Unset, data)

        config = _parse_config(d.pop("config", UNSET))

        def _parse_values(data: object) -> EffectiveLaunchValues | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                values_type_1 = EffectiveLaunchValues.from_dict(data)

                return values_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(EffectiveLaunchValues | None | Unset, data)

        values = _parse_values(d.pop("values", UNSET))

        effective_response = cls(
            config=config,
            values=values,
        )

        effective_response.additional_properties = d
        return effective_response

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
