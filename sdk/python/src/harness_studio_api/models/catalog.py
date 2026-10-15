from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.backend_info import BackendInfo
    from ..models.lemonade_model import LemonadeModel


T = TypeVar("T", bound="Catalog")


@_attrs_define
class Catalog:
    """The registry as of the last time `lemond` ran: lets the gateway list the downloaded models
    and the card show the choices without starting a server.

        Attributes:
            updated_at (int): Unix seconds of the refresh (0 = never).
            models (list[LemonadeModel]):
            backends (list[BackendInfo]):
    """

    updated_at: int
    models: list[LemonadeModel]
    backends: list[BackendInfo]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        updated_at = self.updated_at

        models = []
        for models_item_data in self.models:
            models_item = models_item_data.to_dict()
            models.append(models_item)

        backends = []
        for backends_item_data in self.backends:
            backends_item = backends_item_data.to_dict()
            backends.append(backends_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "updated_at": updated_at,
                "models": models,
                "backends": backends,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.backend_info import BackendInfo  # noqa: PLC0415
        from ..models.lemonade_model import LemonadeModel  # noqa: PLC0415

        d = dict(src_dict)
        updated_at = d.pop("updated_at")

        models = []
        _models = d.pop("models")
        for models_item_data in _models:
            models_item = LemonadeModel.from_dict(models_item_data)

            models.append(models_item)

        backends = []
        _backends = d.pop("backends")
        for backends_item_data in _backends:
            backends_item = BackendInfo.from_dict(backends_item_data)

            backends.append(backends_item)

        catalog = cls(
            updated_at=updated_at,
            models=models,
            backends=backends,
        )

        catalog.additional_properties = d
        return catalog

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
