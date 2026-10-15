from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.error_detail_params import ErrorDetailParams


T = TypeVar("T", bound="ErrorDetail")


@_attrs_define
class ErrorDetail:
    """
    Attributes:
        code (str): Stable machine code (`api.unauthorized`, `network.lan_without_key`, `fs.read_failed`...).
            `core.error` only for the rare core errors that are free text.
        message_key (str): i18n key of the message (Harness Studio locale files; same as `code`).
        params (ErrorDetailParams): Values for the message's `{name}` placeholders (`path`, `error`, `detail`,
            `message`,
            `scope`...). Technical detail (paths, OS error text) is untranslated; a string value may
            itself be a keyed error (`key {"name": "value"}` or `key: detail`), a nested cause.
    """

    code: str
    message_key: str
    params: ErrorDetailParams
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code = self.code

        message_key = self.message_key

        params = self.params.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code": code,
                "message_key": message_key,
                "params": params,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.error_detail_params import ErrorDetailParams  # noqa: PLC0415

        d = dict(src_dict)
        code = d.pop("code")

        message_key = d.pop("message_key")

        params = ErrorDetailParams.from_dict(d.pop("params"))

        error_detail = cls(
            code=code,
            message_key=message_key,
            params=params,
        )

        error_detail.additional_properties = d
        return error_detail

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
