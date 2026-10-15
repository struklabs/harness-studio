from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.recommended_backend import RecommendedBackend, check_recommended_backend
from ..types import UNSET, Unset

T = TypeVar("T", bound="BackendAdvice")


@_attrs_define
class BackendAdvice:
    """Why a backend is recommended, for the UI (`reason_key` is an i18n key under `backend.`).

    Attributes:
        recommended (RecommendedBackend):
        reason_key (str): `backend.reason_nvidia_cuda`, `backend.reason_amd_rocm`, ...
        alternatives (list[RecommendedBackend]): Other official builds that also run on this GPU.
        gpu_name (None | str | Unset): GPU the advice is about.
    """

    recommended: RecommendedBackend
    reason_key: str
    alternatives: list[RecommendedBackend]
    gpu_name: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        recommended: str = self.recommended

        reason_key = self.reason_key

        alternatives = []
        for alternatives_item_data in self.alternatives:
            alternatives_item: str = alternatives_item_data
            alternatives.append(alternatives_item)

        gpu_name: None | str | Unset
        if isinstance(self.gpu_name, Unset):
            gpu_name = UNSET
        else:
            gpu_name = self.gpu_name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "recommended": recommended,
                "reason_key": reason_key,
                "alternatives": alternatives,
            }
        )
        if gpu_name is not UNSET:
            field_dict["gpu_name"] = gpu_name

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        recommended = check_recommended_backend(d.pop("recommended"))

        reason_key = d.pop("reason_key")

        alternatives = []
        _alternatives = d.pop("alternatives")
        for alternatives_item_data in _alternatives:
            alternatives_item = check_recommended_backend(alternatives_item_data)

            alternatives.append(alternatives_item)

        def _parse_gpu_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        gpu_name = _parse_gpu_name(d.pop("gpu_name", UNSET))

        backend_advice = cls(
            recommended=recommended,
            reason_key=reason_key,
            alternatives=alternatives,
            gpu_name=gpu_name,
        )

        backend_advice.additional_properties = d
        return backend_advice

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
