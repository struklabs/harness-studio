from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.compaction_settings import CompactionSettings


T = TypeVar("T", bound="ResponsesSettings")


@_attrs_define
class ResponsesSettings:
    """`settings.json` → `gateway.responses`.

    Attributes:
        store (bool | Unset): Store responses that ask for it (`store` is `true` unless the request says `false`).
            Off = nothing new is stored; already stored responses stay readable (and can be
            continued) until retention drops them or they are cleared. Default: True.
        retention_days (int | Unset): Days a stored response is kept after it was created or last continued. Default:
            30.
        max_stored (int | Unset): Most stored responses kept (all callers together). Default: 1000.
        max_mib (int | Unset): Most bytes kept (JSON of input items and responses), MiB. Default: 100.
        compaction (CompactionSettings | Unset): Context compaction of stored Responses chains
            (`gateway.responses.compaction`; see
            ARCHITECTURE §9.1): when the conversation a `previous_response_id` continues no longer fits
            the model comfortably, its older links are replaced, in the request, by a summary the model
            writes. The summary is stored on the new response (`harness_compaction`), so later turns
            start from it instead of the whole chain. Always opt-in: a request asks for it with
            `context_management: [{"type": "compaction", "compact_threshold": <tokens>}]`, or the
            setting turns it on for every chained request.
    """

    store: bool | Unset = True
    retention_days: int | Unset = 30
    max_stored: int | Unset = 1000
    max_mib: int | Unset = 100
    compaction: CompactionSettings | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        store = self.store

        retention_days = self.retention_days

        max_stored = self.max_stored

        max_mib = self.max_mib

        compaction: dict[str, Any] | Unset = UNSET
        if not isinstance(self.compaction, Unset):
            compaction = self.compaction.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if store is not UNSET:
            field_dict["store"] = store
        if retention_days is not UNSET:
            field_dict["retention_days"] = retention_days
        if max_stored is not UNSET:
            field_dict["max_stored"] = max_stored
        if max_mib is not UNSET:
            field_dict["max_mib"] = max_mib
        if compaction is not UNSET:
            field_dict["compaction"] = compaction

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.compaction_settings import CompactionSettings  # noqa: PLC0415

        d = dict(src_dict)
        store = d.pop("store", UNSET)

        retention_days = d.pop("retention_days", UNSET)

        max_stored = d.pop("max_stored", UNSET)

        max_mib = d.pop("max_mib", UNSET)

        _compaction = d.pop("compaction", UNSET)
        compaction: CompactionSettings | Unset
        if isinstance(_compaction, Unset):
            compaction = UNSET
        else:
            compaction = CompactionSettings.from_dict(_compaction)

        responses_settings = cls(
            store=store,
            retention_days=retention_days,
            max_stored=max_stored,
            max_mib=max_mib,
            compaction=compaction,
        )

        responses_settings.additional_properties = d
        return responses_settings

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
