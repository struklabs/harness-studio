from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.recovery_kind import RecoveryKind, check_recovery_kind
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.recovery_option_overrides import RecoveryOptionOverrides
    from ..models.recovery_option_params import RecoveryOptionParams


T = TypeVar("T", bound="RecoveryOption")


@_attrs_define
class RecoveryOption:
    """One way to retry.

    Attributes:
        id (str): Unique within a plan (`lower_gpu_layers`, `unload_server:m-qwen`...).
        kind (RecoveryKind):
        label_key (str): i18n key (`recovery.lower_gpu_layers`...).
        params (RecoveryOptionParams): Values for the label (`from`, `to`, `total`, `server`, `saved_mib`...).
        overrides (RecoveryOptionOverrides): Transient layer for the retry (catalog ids).
        unload (list[str]): Servers to stop before the retry.
        recommended (bool): The estimate says this alone should make the model fit.
        warning_key (None | str | Unset): i18n key of a caveat shown with the option (`recovery.ram_insufficient`), its
            values
            are in `params`.
    """

    id: str
    kind: RecoveryKind
    label_key: str
    params: RecoveryOptionParams
    overrides: RecoveryOptionOverrides
    unload: list[str]
    recommended: bool
    warning_key: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        kind: str = self.kind

        label_key = self.label_key

        params = self.params.to_dict()

        overrides = self.overrides.to_dict()

        unload = self.unload

        recommended = self.recommended

        warning_key: None | str | Unset
        if isinstance(self.warning_key, Unset):
            warning_key = UNSET
        else:
            warning_key = self.warning_key

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "kind": kind,
                "label_key": label_key,
                "params": params,
                "overrides": overrides,
                "unload": unload,
                "recommended": recommended,
            }
        )
        if warning_key is not UNSET:
            field_dict["warning_key"] = warning_key

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.recovery_option_overrides import (
            RecoveryOptionOverrides,  # noqa: PLC0415
        )
        from ..models.recovery_option_params import (
            RecoveryOptionParams,  # noqa: PLC0415
        )

        d = dict(src_dict)
        id = d.pop("id")

        kind = check_recovery_kind(d.pop("kind"))

        label_key = d.pop("label_key")

        params = RecoveryOptionParams.from_dict(d.pop("params"))

        overrides = RecoveryOptionOverrides.from_dict(d.pop("overrides"))

        unload = cast(list[str], d.pop("unload"))

        recommended = d.pop("recommended")

        def _parse_warning_key(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        warning_key = _parse_warning_key(d.pop("warning_key", UNSET))

        recovery_option = cls(
            id=id,
            kind=kind,
            label_key=label_key,
            params=params,
            overrides=overrides,
            unload=unload,
            recommended=recommended,
            warning_key=warning_key,
        )

        recovery_option.additional_properties = d
        return recovery_option

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
