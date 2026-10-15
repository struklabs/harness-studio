from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ort_variant import OrtVariant, check_ort_variant
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.installed import Installed


T = TypeVar("T", bound="VariantView")


@_attrs_define
class VariantView:
    """Settings > Runtime view: every build of this platform with its state.

    Attributes:
        variant (OrtVariant): Which ONNX Runtime build.
        version (str):
        download_bytes (int): Download size of the pinned archives (the real transfer is smaller for ZIPs).
        default (bool):
        installed (Installed | None | Unset):
    """

    variant: OrtVariant
    version: str
    download_bytes: int
    default: bool
    installed: Installed | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.installed import Installed  # noqa: PLC0415

        variant: str = self.variant

        version = self.version

        download_bytes = self.download_bytes

        default = self.default

        installed: dict[str, Any] | None | Unset
        if isinstance(self.installed, Unset):
            installed = UNSET
        elif isinstance(self.installed, Installed):
            installed = self.installed.to_dict()
        else:
            installed = self.installed

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "variant": variant,
                "version": version,
                "download_bytes": download_bytes,
                "default": default,
            }
        )
        if installed is not UNSET:
            field_dict["installed"] = installed

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.installed import Installed  # noqa: PLC0415

        d = dict(src_dict)
        variant = check_ort_variant(d.pop("variant"))

        version = d.pop("version")

        download_bytes = d.pop("download_bytes")

        default = d.pop("default")

        def _parse_installed(data: object) -> Installed | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                installed_type_1 = Installed.from_dict(data)

                return installed_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Installed | None | Unset, data)

        installed = _parse_installed(d.pop("installed", UNSET))

        variant_view = cls(
            variant=variant,
            version=version,
            download_bytes=download_bytes,
            default=default,
            installed=installed,
        )

        variant_view.additional_properties = d
        return variant_view

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
