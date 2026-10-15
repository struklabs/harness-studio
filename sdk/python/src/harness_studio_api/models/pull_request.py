from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="PullRequest")


@_attrs_define
class PullRequest:
    """
    Attributes:
        repo (str): Hugging Face repo (`owner/name`).
        file (None | str | Unset): A file of the variant (or the variant name). Takes precedence over `quant`.
        quant (None | str | Unset): Quantization (`Q4_K_M`...), case-insensitive.
    """

    repo: str
    file: None | str | Unset = UNSET
    quant: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo = self.repo

        file: None | str | Unset
        if isinstance(self.file, Unset):
            file = UNSET
        else:
            file = self.file

        quant: None | str | Unset
        if isinstance(self.quant, Unset):
            quant = UNSET
        else:
            quant = self.quant

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo": repo,
            }
        )
        if file is not UNSET:
            field_dict["file"] = file
        if quant is not UNSET:
            field_dict["quant"] = quant

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        repo = d.pop("repo")

        def _parse_file(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        file = _parse_file(d.pop("file", UNSET))

        def _parse_quant(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        quant = _parse_quant(d.pop("quant", UNSET))

        pull_request = cls(
            repo=repo,
            file=file,
            quant=quant,
        )

        pull_request.additional_properties = d
        return pull_request

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
