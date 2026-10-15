from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ConvertRequest")


@_attrs_define
class ConvertRequest:
    """
    Attributes:
        repo (str): Hugging Face repo (`owner/name`) with safetensors weights.
        quant (None | str | Unset): Target type (`Q4_K_M` default, `Q5_K_M`, `Q6_K`, `Q8_0`, `F16`, `BF16`, or any type
            the runtime's `llama-quantize` lists).
        keep_original (bool | Unset): Keep the downloaded safetensors in the library after the conversion.
    """

    repo: str
    quant: None | str | Unset = UNSET
    keep_original: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo = self.repo

        quant: None | str | Unset
        if isinstance(self.quant, Unset):
            quant = UNSET
        else:
            quant = self.quant

        keep_original = self.keep_original

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo": repo,
            }
        )
        if quant is not UNSET:
            field_dict["quant"] = quant
        if keep_original is not UNSET:
            field_dict["keep_original"] = keep_original

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        repo = d.pop("repo")

        def _parse_quant(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        quant = _parse_quant(d.pop("quant", UNSET))

        keep_original = d.pop("keep_original", UNSET)

        convert_request = cls(
            repo=repo,
            quant=quant,
            keep_original=keep_original,
        )

        convert_request.additional_properties = d
        return convert_request

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
