from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.request_row import RequestRow


T = TypeVar("T", bound="RequestPage")


@_attrs_define
class RequestPage:
    """
    Attributes:
        requests (list[RequestRow]):
        total (int): Rows matching the filters (all pages).
        models (list[str]): Values present in the stored rows, for filter pickers.
        clients (list[str]):
        endpoints (list[str]):
    """

    requests: list[RequestRow]
    total: int
    models: list[str]
    clients: list[str]
    endpoints: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        requests = []
        for requests_item_data in self.requests:
            requests_item = requests_item_data.to_dict()
            requests.append(requests_item)

        total = self.total

        models = self.models

        clients = self.clients

        endpoints = self.endpoints

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "requests": requests,
                "total": total,
                "models": models,
                "clients": clients,
                "endpoints": endpoints,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.request_row import RequestRow  # noqa: PLC0415

        d = dict(src_dict)
        requests = []
        _requests = d.pop("requests")
        for requests_item_data in _requests:
            requests_item = RequestRow.from_dict(requests_item_data)

            requests.append(requests_item)

        total = d.pop("total")

        models = cast(list[str], d.pop("models"))

        clients = cast(list[str], d.pop("clients"))

        endpoints = cast(list[str], d.pop("endpoints"))

        request_page = cls(
            requests=requests,
            total=total,
            models=models,
            clients=clients,
            endpoints=endpoints,
        )

        request_page.additional_properties = d
        return request_page

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
