from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.compaction_marker import CompactionMarker


T = TypeVar("T", bound="CompactResponse")


@_attrs_define
class CompactResponse:
    """Result of a compaction.

    Attributes:
        conversation_id (str):
        marker_id (str): Id of the marker message (undo it with `PUT .../compactions/{marker_id}`).
        marker (CompactionMarker): The compaction marker stored in `meta.compaction` of the marker message's variant.
        requests (int): Summarization requests made (chunks).
    """

    conversation_id: str
    marker_id: str
    marker: CompactionMarker
    requests: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        conversation_id = self.conversation_id

        marker_id = self.marker_id

        marker = self.marker.to_dict()

        requests = self.requests

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "conversation_id": conversation_id,
                "marker_id": marker_id,
                "marker": marker,
                "requests": requests,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.compaction_marker import CompactionMarker  # noqa: PLC0415

        d = dict(src_dict)
        conversation_id = d.pop("conversation_id")

        marker_id = d.pop("marker_id")

        marker = CompactionMarker.from_dict(d.pop("marker"))

        requests = d.pop("requests")

        compact_response = cls(
            conversation_id=conversation_id,
            marker_id=marker_id,
            marker=marker,
            requests=requests,
        )

        compact_response.additional_properties = d
        return compact_response

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
