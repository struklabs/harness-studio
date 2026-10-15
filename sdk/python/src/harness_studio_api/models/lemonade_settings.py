from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="LemonadeSettings")


@_attrs_define
class LemonadeSettings:
    """Harness's settings of the Lemonade engine (`<data>/lemonade/settings.json`).

    Attributes:
        backend (str | Unset): llama.cpp backend Lemonade uses: `auto` (Lemonade picks), `vulkan`, `rocm`, `cpu`...
        runtime (None | str | Unset): Registry id of the Lemonade build to run (`None` = the newest installed one).
            Pinning
            an older one is the rollback.
    """

    backend: str | Unset = UNSET
    runtime: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        backend = self.backend

        runtime: None | str | Unset
        if isinstance(self.runtime, Unset):
            runtime = UNSET
        else:
            runtime = self.runtime

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if backend is not UNSET:
            field_dict["backend"] = backend
        if runtime is not UNSET:
            field_dict["runtime"] = runtime

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        backend = d.pop("backend", UNSET)

        def _parse_runtime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        runtime = _parse_runtime(d.pop("runtime", UNSET))

        lemonade_settings = cls(
            backend=backend,
            runtime=runtime,
        )

        lemonade_settings.additional_properties = d
        return lemonade_settings

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
