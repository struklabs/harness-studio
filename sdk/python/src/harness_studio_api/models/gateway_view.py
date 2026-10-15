from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.gateway_settings import GatewaySettings
    from ..models.gateway_status import GatewayStatus
    from ..models.model_slot import ModelSlot


T = TypeVar("T", bound="GatewayView")


@_attrs_define
class GatewayView:
    """
    Attributes:
        status (GatewayStatus): What the gateway listener is doing right now (process-wide; set by the HTTP side).
        settings (GatewaySettings): `settings.json` → `gateway`. Changed only through [`validate_settings`] (the general
            settings save keeps the stored block, like `control_api`).
        models (list[ModelSlot]): Definitions the scheduler knows (loaded, loading or used since the gateway started).
    """

    status: GatewayStatus
    settings: GatewaySettings
    models: list[ModelSlot]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        status = self.status.to_dict()

        settings = self.settings.to_dict()

        models = []
        for models_item_data in self.models:
            models_item = models_item_data.to_dict()
            models.append(models_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "status": status,
                "settings": settings,
                "models": models,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gateway_settings import GatewaySettings  # noqa: PLC0415
        from ..models.gateway_status import GatewayStatus  # noqa: PLC0415
        from ..models.model_slot import ModelSlot  # noqa: PLC0415

        d = dict(src_dict)
        status = GatewayStatus.from_dict(d.pop("status"))

        settings = GatewaySettings.from_dict(d.pop("settings"))

        models = []
        _models = d.pop("models")
        for models_item_data in _models:
            models_item = ModelSlot.from_dict(models_item_data)

            models.append(models_item)

        gateway_view = cls(
            status=status,
            settings=settings,
            models=models,
        )

        gateway_view.additional_properties = d
        return gateway_view

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
