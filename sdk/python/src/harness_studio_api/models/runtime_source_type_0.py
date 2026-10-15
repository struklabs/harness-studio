from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.runtime_source_type_0_kind import (
    RuntimeSourceType0Kind,
    check_runtime_source_type_0_kind,
)

if TYPE_CHECKING:
    from ..models.installed_asset import InstalledAsset


T = TypeVar("T", bound="RuntimeSourceType0")


@_attrs_define
class RuntimeSourceType0:
    """A GitHub release archive (`owner/repo`, release tag, installed archives).

    Attributes:
        repo (str):
        tag (str):
        assets (list[InstalledAsset]):
        kind (RuntimeSourceType0Kind):
    """

    repo: str
    tag: str
    assets: list[InstalledAsset]
    kind: RuntimeSourceType0Kind
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo = self.repo

        tag = self.tag

        assets = []
        for assets_item_data in self.assets:
            assets_item = assets_item_data.to_dict()
            assets.append(assets_item)

        kind: str = self.kind

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo": repo,
                "tag": tag,
                "assets": assets,
                "kind": kind,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.installed_asset import InstalledAsset  # noqa: PLC0415

        d = dict(src_dict)
        repo = d.pop("repo")

        tag = d.pop("tag")

        assets = []
        _assets = d.pop("assets")
        for assets_item_data in _assets:
            assets_item = InstalledAsset.from_dict(assets_item_data)

            assets.append(assets_item)

        kind = check_runtime_source_type_0_kind(d.pop("kind"))

        runtime_source_type_0 = cls(
            repo=repo,
            tag=tag,
            assets=assets,
            kind=kind,
        )

        runtime_source_type_0.additional_properties = d
        return runtime_source_type_0

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
