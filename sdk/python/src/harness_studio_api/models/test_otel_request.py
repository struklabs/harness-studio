from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.otel_settings import OtelSettings


T = TypeVar("T", bound="TestOtelRequest")


@_attrs_define
class TestOtelRequest:
    """
    Attributes:
        settings (None | OtelSettings | Unset):
        headers (None | str | Unset): Headers to test with (unsaved); omitted = the saved ones.
    """

    settings: None | OtelSettings | Unset = UNSET
    headers: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.otel_settings import OtelSettings  # noqa: PLC0415

        settings: dict[str, Any] | None | Unset
        if isinstance(self.settings, Unset):
            settings = UNSET
        elif isinstance(self.settings, OtelSettings):
            settings = self.settings.to_dict()
        else:
            settings = self.settings

        headers: None | str | Unset
        if isinstance(self.headers, Unset):
            headers = UNSET
        else:
            headers = self.headers

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if settings is not UNSET:
            field_dict["settings"] = settings
        if headers is not UNSET:
            field_dict["headers"] = headers

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.otel_settings import OtelSettings  # noqa: PLC0415

        d = dict(src_dict)

        def _parse_settings(data: object) -> None | OtelSettings | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                settings_type_1 = OtelSettings.from_dict(data)

                return settings_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | OtelSettings | Unset, data)

        settings = _parse_settings(d.pop("settings", UNSET))

        def _parse_headers(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        headers = _parse_headers(d.pop("headers", UNSET))

        test_otel_request = cls(
            settings=settings,
            headers=headers,
        )

        test_otel_request.additional_properties = d
        return test_otel_request

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
