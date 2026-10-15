from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.release_pick_assets_item import ReleasePickAssetsItem


T = TypeVar("T", bound="ReleasePick")


@_attrs_define
class ReleasePick:
    """The archive(s) a query resolves to, shown before anything is downloaded.

    Attributes:
        family (str): Runtime family the archives belong to.
        repo (str):
        tag (str):
        assets (list[ReleasePickAssetsItem]): Main archive first, then companions (CUDA runtime DLLs).
        unverified (list[str]): Names of archives without a usable SHA-256 digest (installing needs confirmation).
        published_at (None | str | Unset):
        backend (None | str | Unset): Backend token of the chosen archive (`None` = plain build / explicit asset).
    """

    family: str
    repo: str
    tag: str
    assets: list[ReleasePickAssetsItem]
    unverified: list[str]
    published_at: None | str | Unset = UNSET
    backend: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        family = self.family

        repo = self.repo

        tag = self.tag

        assets = []
        for assets_item_data in self.assets:
            assets_item = assets_item_data.to_dict()
            assets.append(assets_item)

        unverified = self.unverified

        published_at: None | str | Unset
        if isinstance(self.published_at, Unset):
            published_at = UNSET
        else:
            published_at = self.published_at

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "family": family,
                "repo": repo,
                "tag": tag,
                "assets": assets,
                "unverified": unverified,
            }
        )
        if published_at is not UNSET:
            field_dict["published_at"] = published_at
        if backend is not UNSET:
            field_dict["backend"] = backend

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.release_pick_assets_item import (
            ReleasePickAssetsItem,  # noqa: PLC0415
        )

        d = dict(src_dict)
        family = d.pop("family")

        repo = d.pop("repo")

        tag = d.pop("tag")

        assets = []
        _assets = d.pop("assets")
        for assets_item_data in _assets:
            assets_item = ReleasePickAssetsItem.from_dict(assets_item_data)

            assets.append(assets_item)

        unverified = cast(list[str], d.pop("unverified"))

        def _parse_published_at(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        published_at = _parse_published_at(d.pop("published_at", UNSET))

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        release_pick = cls(
            family=family,
            repo=repo,
            tag=tag,
            assets=assets,
            unverified=unverified,
            published_at=published_at,
            backend=backend,
        )

        release_pick.additional_properties = d
        return release_pick

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
