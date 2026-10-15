from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.otel_settings import OtelSettings


T = TypeVar("T", bound="SaveOtelRequest")


@_attrs_define
class SaveOtelRequest:
    """
    Attributes:
        settings (OtelSettings): `settings.json` → `otel`. Changed only through [`save_settings`] (the general settings
            save keeps the stored block, like `gateway` and `control_api`).
        headers (None | str | Unset): Export headers, one `name=value` (or `name: value`) per line. Omitted/null = keep
            the
            saved ones, `""` = remove them.
    """

    settings: OtelSettings
    headers: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        settings = self.settings.to_dict()

        headers: None | str | Unset
        if isinstance(self.headers, Unset):
            headers = UNSET
        else:
            headers = self.headers

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "settings": settings,
            }
        )
        if headers is not UNSET:
            field_dict["headers"] = headers

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.otel_settings import OtelSettings  # noqa: PLC0415

        d = dict(src_dict)
        settings = OtelSettings.from_dict(d.pop("settings"))

        def _parse_headers(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        headers = _parse_headers(d.pop("headers", UNSET))

        save_otel_request = cls(
            settings=settings,
            headers=headers,
        )

        save_otel_request.additional_properties = d
        return save_otel_request

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
