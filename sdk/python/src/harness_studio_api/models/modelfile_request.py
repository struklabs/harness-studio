from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ModelfileRequest")


@_attrs_define
class ModelfileRequest:
    """Request of the Control API / GUI: a Modelfile's text and where its relative paths start.

    Attributes:
        text (str):
        base_dir (None | str | Unset): Directory of the Modelfile (relative `FROM` / `ADAPTER` paths).
        alias (None | str | Unset): Alias to use (default: derived from `FROM`).
    """

    text: str
    base_dir: None | str | Unset = UNSET
    alias: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        text = self.text

        base_dir: None | str | Unset
        if isinstance(self.base_dir, Unset):
            base_dir = UNSET
        else:
            base_dir = self.base_dir

        alias: None | str | Unset
        if isinstance(self.alias, Unset):
            alias = UNSET
        else:
            alias = self.alias

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "text": text,
            }
        )
        if base_dir is not UNSET:
            field_dict["base_dir"] = base_dir
        if alias is not UNSET:
            field_dict["alias"] = alias

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        text = d.pop("text")

        def _parse_base_dir(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        base_dir = _parse_base_dir(d.pop("base_dir", UNSET))

        def _parse_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        alias = _parse_alias(d.pop("alias", UNSET))

        modelfile_request = cls(
            text=text,
            base_dir=base_dir,
            alias=alias,
        )

        modelfile_request.additional_properties = d
        return modelfile_request

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
