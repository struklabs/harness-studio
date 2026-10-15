from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ReleaseQuery")


@_attrs_define
class ReleaseQuery:
    """What to install from a repository.

    Attributes:
        repo (str): `owner/repo` (e.g. `PrismML-Eng/llama.cpp`).
        family (str | Unset): Runtime family whose release naming picks the archive (default: `llama.cpp`).
        tag (None | str | Unset): Release tag (default: the newest release that has a matching archive).
        asset (None | str | Unset): Exact archive name (default: picked for this OS/arch and `backend`).
        backend (None | str | Unset): `vulkan`, `cuda`, `rocm`, `sycl`, `cpu`, `metal` (default: the hardware
            recommendation, then Vulkan, then CPU).
    """

    repo: str
    family: str | Unset = UNSET
    tag: None | str | Unset = UNSET
    asset: None | str | Unset = UNSET
    backend: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo = self.repo

        family = self.family

        tag: None | str | Unset
        if isinstance(self.tag, Unset):
            tag = UNSET
        else:
            tag = self.tag

        asset: None | str | Unset
        if isinstance(self.asset, Unset):
            asset = UNSET
        else:
            asset = self.asset

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo": repo,
            }
        )
        if family is not UNSET:
            field_dict["family"] = family
        if tag is not UNSET:
            field_dict["tag"] = tag
        if asset is not UNSET:
            field_dict["asset"] = asset
        if backend is not UNSET:
            field_dict["backend"] = backend

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        repo = d.pop("repo")

        family = d.pop("family", UNSET)

        def _parse_tag(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tag = _parse_tag(d.pop("tag", UNSET))

        def _parse_asset(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        asset = _parse_asset(d.pop("asset", UNSET))

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        release_query = cls(
            repo=repo,
            family=family,
            tag=tag,
            asset=asset,
            backend=backend,
        )

        release_query.additional_properties = d
        return release_query

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
