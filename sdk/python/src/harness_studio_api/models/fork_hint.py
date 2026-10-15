from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="ForkHint")


@_attrs_define
class ForkHint:
    """A fork that adds tensor types of its own.

    Attributes:
        id (str): Stable id (`prismml`); also the suggested runtime id.
        name (str):
        repo (str): GitHub `owner/repo` that publishes the release archives.
        url (str):
    """

    id: str
    name: str
    repo: str
    url: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        repo = self.repo

        url = self.url

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "repo": repo,
                "url": url,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        repo = d.pop("repo")

        url = d.pop("url")

        fork_hint = cls(
            id=id,
            name=name,
            repo=repo,
            url=url,
        )

        fork_hint.additional_properties = d
        return fork_hint

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
