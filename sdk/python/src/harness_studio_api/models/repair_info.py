from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RepairInfo")


@_attrs_define
class RepairInfo:
    """Where a missing part could come from.

    Attributes:
        repo_id (None | str | Unset): Hugging Face repo, known from the download layout `<library>/<owner>/<repo>/...`.
        target_dir (None | str | Unset): Folder the parts go to (`<library>/<owner>/<repo>`).
        reason (None | str | Unset): Why it cannot be repaired automatically (`None` = it can).
        source_url (None | str | Unset): Address of a part that is still there, recorded when the file was downloaded
            (custom
            URL, or a Hugging Face file that no longer sits in its repo folder). The missing parts
            are fetched from the same address with the part number replaced.
        url_allowed (bool | Unset): The user can give the address of one part to repair with (the file is in a library
            folder and not linked): offered when there is neither a repo folder nor a record.
    """

    repo_id: None | str | Unset = UNSET
    target_dir: None | str | Unset = UNSET
    reason: None | str | Unset = UNSET
    source_url: None | str | Unset = UNSET
    url_allowed: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo_id: None | str | Unset
        if isinstance(self.repo_id, Unset):
            repo_id = UNSET
        else:
            repo_id = self.repo_id

        target_dir: None | str | Unset
        if isinstance(self.target_dir, Unset):
            target_dir = UNSET
        else:
            target_dir = self.target_dir

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

        source_url: None | str | Unset
        if isinstance(self.source_url, Unset):
            source_url = UNSET
        else:
            source_url = self.source_url

        url_allowed = self.url_allowed

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if repo_id is not UNSET:
            field_dict["repo_id"] = repo_id
        if target_dir is not UNSET:
            field_dict["target_dir"] = target_dir
        if reason is not UNSET:
            field_dict["reason"] = reason
        if source_url is not UNSET:
            field_dict["source_url"] = source_url
        if url_allowed is not UNSET:
            field_dict["url_allowed"] = url_allowed

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_repo_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        repo_id = _parse_repo_id(d.pop("repo_id", UNSET))

        def _parse_target_dir(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        target_dir = _parse_target_dir(d.pop("target_dir", UNSET))

        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))

        def _parse_source_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source_url = _parse_source_url(d.pop("source_url", UNSET))

        url_allowed = d.pop("url_allowed", UNSET)

        repair_info = cls(
            repo_id=repo_id,
            target_dir=target_dir,
            reason=reason,
            source_url=source_url,
            url_allowed=url_allowed,
        )

        repair_info.additional_properties = d
        return repair_info

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
