from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.ort_variant import OrtVariant, check_ort_variant

if TYPE_CHECKING:
    from ..models.installed_file import InstalledFile


T = TypeVar("T", bound="Installed")


@_attrs_define
class Installed:
    """An installed build.

    Attributes:
        variant (OrtVariant): Which ONNX Runtime build.
        version (str):
        dir_ (str):
        lib (str): The library the sidecar loads.
        files (list[InstalledFile]):
        size_bytes (int):
        installed_at (int):
    """

    variant: OrtVariant
    version: str
    dir_: str
    lib: str
    files: list[InstalledFile]
    size_bytes: int
    installed_at: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        variant: str = self.variant

        version = self.version

        dir_ = self.dir_

        lib = self.lib

        files = []
        for files_item_data in self.files:
            files_item = files_item_data.to_dict()
            files.append(files_item)

        size_bytes = self.size_bytes

        installed_at = self.installed_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "variant": variant,
                "version": version,
                "dir": dir_,
                "lib": lib,
                "files": files,
                "size_bytes": size_bytes,
                "installed_at": installed_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.installed_file import InstalledFile  # noqa: PLC0415

        d = dict(src_dict)
        variant = check_ort_variant(d.pop("variant"))

        version = d.pop("version")

        dir_ = d.pop("dir")

        lib = d.pop("lib")

        files = []
        _files = d.pop("files")
        for files_item_data in _files:
            files_item = InstalledFile.from_dict(files_item_data)

            files.append(files_item)

        size_bytes = d.pop("size_bytes")

        installed_at = d.pop("installed_at")

        installed = cls(
            variant=variant,
            version=version,
            dir_=dir_,
            lib=lib,
            files=files,
            size_bytes=size_bytes,
            installed_at=installed_at,
        )

        installed.additional_properties = d
        return installed

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
