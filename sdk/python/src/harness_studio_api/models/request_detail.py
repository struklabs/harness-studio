from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.phase import Phase
    from ..models.request_row import RequestRow
    from ..models.stored_content import StoredContent


T = TypeVar("T", bound="RequestDetail")


@_attrs_define
class RequestDetail:
    """`GET /analytics/requests/{id}`.

    Attributes:
        request (RequestRow): One stored gateway request (metadata only).
        timeline (list[Phase]): load → queue → prompt → generation → other, in ms from arrival.
        spec_accept_rate (float | None | Unset):
        content (None | StoredContent | Unset):
    """

    request: RequestRow
    timeline: list[Phase]
    spec_accept_rate: float | None | Unset = UNSET
    content: None | StoredContent | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.stored_content import StoredContent  # noqa: PLC0415

        request = self.request.to_dict()

        timeline = []
        for timeline_item_data in self.timeline:
            timeline_item = timeline_item_data.to_dict()
            timeline.append(timeline_item)

        spec_accept_rate: float | None | Unset
        if isinstance(self.spec_accept_rate, Unset):
            spec_accept_rate = UNSET
        else:
            spec_accept_rate = self.spec_accept_rate

        content: dict[str, Any] | None | Unset
        if isinstance(self.content, Unset):
            content = UNSET
        elif isinstance(self.content, StoredContent):
            content = self.content.to_dict()
        else:
            content = self.content

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "request": request,
                "timeline": timeline,
            }
        )
        if spec_accept_rate is not UNSET:
            field_dict["spec_accept_rate"] = spec_accept_rate
        if content is not UNSET:
            field_dict["content"] = content

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.phase import Phase  # noqa: PLC0415
        from ..models.request_row import RequestRow  # noqa: PLC0415
        from ..models.stored_content import StoredContent  # noqa: PLC0415

        d = dict(src_dict)
        request = RequestRow.from_dict(d.pop("request"))

        timeline = []
        _timeline = d.pop("timeline")
        for timeline_item_data in _timeline:
            timeline_item = Phase.from_dict(timeline_item_data)

            timeline.append(timeline_item)

        def _parse_spec_accept_rate(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        spec_accept_rate = _parse_spec_accept_rate(d.pop("spec_accept_rate", UNSET))

        def _parse_content(data: object) -> None | StoredContent | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                content_type_1 = StoredContent.from_dict(data)

                return content_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | StoredContent | Unset, data)

        content = _parse_content(d.pop("content", UNSET))

        request_detail = cls(
            request=request,
            timeline=timeline,
            spec_accept_rate=spec_accept_rate,
            content=content,
        )

        request_detail.additional_properties = d
        return request_detail

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
