from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ort_variant import OrtVariant, check_ort_variant
from ..types import UNSET, Unset

T = TypeVar("T", bound="InstallProgress")


@_attrs_define
class InstallProgress:
    """Download progress of the running install (`None` when idle).

    Attributes:
        current (str):
        downloaded_bytes (int):
        total_bytes (int):
        variant (None | OrtVariant | Unset):
    """

    current: str
    downloaded_bytes: int
    total_bytes: int
    variant: None | OrtVariant | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        current = self.current

        downloaded_bytes = self.downloaded_bytes

        total_bytes = self.total_bytes

        variant: None | str | Unset
        if isinstance(self.variant, Unset):
            variant = UNSET
        elif isinstance(self.variant, str):
            variant = self.variant
        else:
            variant = self.variant

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "current": current,
                "downloaded_bytes": downloaded_bytes,
                "total_bytes": total_bytes,
            }
        )
        if variant is not UNSET:
            field_dict["variant"] = variant

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        current = d.pop("current")

        downloaded_bytes = d.pop("downloaded_bytes")

        total_bytes = d.pop("total_bytes")

        def _parse_variant(data: object) -> None | OrtVariant | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                variant_type_1 = check_ort_variant(data)

                return variant_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | OrtVariant | Unset, data)

        variant = _parse_variant(d.pop("variant", UNSET))

        install_progress = cls(
            current=current,
            downloaded_bytes=downloaded_bytes,
            total_bytes=total_bytes,
            variant=variant,
        )

        install_progress.additional_properties = d
        return install_progress

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
