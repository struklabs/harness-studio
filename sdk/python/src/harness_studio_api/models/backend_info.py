from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="BackendInfo")


@_attrs_define
class BackendInfo:
    """A backend Lemonade can run models on, from `GET /v1/system-info` `recipes`.

    Attributes:
        recipe (str): Recipe: `llamacpp`, `flm`, `whispercpp`...
        backend (str): `vulkan`, `rocm`, `cpu`, `npu`...
        state (str): `installed`, `installable`, `update_required`, `unsupported`...
        version (None | str | Unset): Release tag Lemonade pins for this backend (`b10825`).
        message (None | str | Unset): Why it is not usable on this machine (`Unsupported GPU`).
    """

    recipe: str
    backend: str
    state: str
    version: None | str | Unset = UNSET
    message: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        recipe = self.recipe

        backend = self.backend

        state = self.state

        version: None | str | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        else:
            version = self.version

        message: None | str | Unset
        if isinstance(self.message, Unset):
            message = UNSET
        else:
            message = self.message

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "recipe": recipe,
                "backend": backend,
                "state": state,
            }
        )
        if version is not UNSET:
            field_dict["version"] = version
        if message is not UNSET:
            field_dict["message"] = message

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        recipe = d.pop("recipe")

        backend = d.pop("backend")

        state = d.pop("state")

        def _parse_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        version = _parse_version(d.pop("version", UNSET))

        def _parse_message(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        message = _parse_message(d.pop("message", UNSET))

        backend_info = cls(
            recipe=recipe,
            backend=backend,
            state=state,
            version=version,
            message=message,
        )

        backend_info.additional_properties = d
        return backend_info

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
