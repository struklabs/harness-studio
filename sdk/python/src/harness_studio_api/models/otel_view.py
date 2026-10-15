from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.otel_settings import OtelSettings
    from ..models.otel_status import OtelStatus


T = TypeVar("T", bound="OtelView")


@_attrs_define
class OtelView:
    """
    Attributes:
        settings (OtelSettings): `settings.json` → `otel`. Changed only through [`save_settings`] (the general settings
            save keeps the stored block, like `gateway` and `control_api`).
        status (OtelStatus): Exporter state for Settings and diagnostics.
    """

    settings: OtelSettings
    status: OtelStatus
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        settings = self.settings.to_dict()

        status = self.status.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "settings": settings,
                "status": status,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.otel_settings import OtelSettings  # noqa: PLC0415
        from ..models.otel_status import OtelStatus  # noqa: PLC0415

        d = dict(src_dict)
        settings = OtelSettings.from_dict(d.pop("settings"))

        status = OtelStatus.from_dict(d.pop("status"))

        otel_view = cls(
            settings=settings,
            status=status,
        )

        otel_view.additional_properties = d
        return otel_view

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
