from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="CompactionSettings")


@_attrs_define
class CompactionSettings:
    """Context compaction of stored Responses chains (`gateway.responses.compaction`; see
    ARCHITECTURE §9.1): when the conversation a `previous_response_id` continues no longer fits
    the model comfortably, its older links are replaced, in the request, by a summary the model
    writes. The summary is stored on the new response (`harness_compaction`), so later turns
    start from it instead of the whole chain. Always opt-in: a request asks for it with
    `context_management: [{"type": "compaction", "compact_threshold": <tokens>}]`, or the
    setting turns it on for every chained request.

        Attributes:
            auto (bool | Unset): Compact every chained request that crosses the threshold, not only the ones that ask.
                Default: False.
            threshold_percent (int | Unset): Share of the model's context window (per slot) the conversation may fill before
                it is
                compacted. Default: 80.
            keep_links (int | Unset): Newest links kept as they are (their input and output); older ones are summarized.
                Default: 2.
    """

    auto: bool | Unset = False
    threshold_percent: int | Unset = 80
    keep_links: int | Unset = 2
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        auto = self.auto

        threshold_percent = self.threshold_percent

        keep_links = self.keep_links

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if auto is not UNSET:
            field_dict["auto"] = auto
        if threshold_percent is not UNSET:
            field_dict["threshold_percent"] = threshold_percent
        if keep_links is not UNSET:
            field_dict["keep_links"] = keep_links

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        auto = d.pop("auto", UNSET)

        threshold_percent = d.pop("threshold_percent", UNSET)

        keep_links = d.pop("keep_links", UNSET)

        compaction_settings = cls(
            auto=auto,
            threshold_percent=threshold_percent,
            keep_links=keep_links,
        )

        compaction_settings.additional_properties = d
        return compaction_settings

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
