from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.managed_runtime_status_backup_type_0 import (
        ManagedRuntimeStatusBackupType0,
    )
    from ..models.managed_runtime_status_download_type_0 import (
        ManagedRuntimeStatusDownloadType0,
    )


T = TypeVar("T", bound="ManagedRuntimeStatus")


@_attrs_define
class ManagedRuntimeStatus:
    """Settings > Runtime's managed build, for the Control API and the CLI.

    Attributes:
        auto_update (bool):
        installed_version (None | str | Unset):
        previous_version (None | str | Unset):
        backend (None | str | Unset): Backend recorded for the installed build (what updates keep).
        installed_backend (None | str | Unset): Backend detected from the installed build's files.
        backup (ManagedRuntimeStatusBackupType0 | None | Unset): What "roll back" restores (`None` = the backup slot
            holds no build).
        download (ManagedRuntimeStatusDownloadType0 | None | Unset): Download of a running install (`None` when idle).
    """

    auto_update: bool
    installed_version: None | str | Unset = UNSET
    previous_version: None | str | Unset = UNSET
    backend: None | str | Unset = UNSET
    installed_backend: None | str | Unset = UNSET
    backup: ManagedRuntimeStatusBackupType0 | None | Unset = UNSET
    download: ManagedRuntimeStatusDownloadType0 | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.managed_runtime_status_backup_type_0 import (
            ManagedRuntimeStatusBackupType0,  # noqa: PLC0415
        )
        from ..models.managed_runtime_status_download_type_0 import (
            ManagedRuntimeStatusDownloadType0,  # noqa: PLC0415
        )

        auto_update = self.auto_update

        installed_version: None | str | Unset
        if isinstance(self.installed_version, Unset):
            installed_version = UNSET
        else:
            installed_version = self.installed_version

        previous_version: None | str | Unset
        if isinstance(self.previous_version, Unset):
            previous_version = UNSET
        else:
            previous_version = self.previous_version

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        installed_backend: None | str | Unset
        if isinstance(self.installed_backend, Unset):
            installed_backend = UNSET
        else:
            installed_backend = self.installed_backend

        backup: dict[str, Any] | None | Unset
        if isinstance(self.backup, Unset):
            backup = UNSET
        elif isinstance(self.backup, ManagedRuntimeStatusBackupType0):
            backup = self.backup.to_dict()
        else:
            backup = self.backup

        download: dict[str, Any] | None | Unset
        if isinstance(self.download, Unset):
            download = UNSET
        elif isinstance(self.download, ManagedRuntimeStatusDownloadType0):
            download = self.download.to_dict()
        else:
            download = self.download

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "auto_update": auto_update,
            }
        )
        if installed_version is not UNSET:
            field_dict["installed_version"] = installed_version
        if previous_version is not UNSET:
            field_dict["previous_version"] = previous_version
        if backend is not UNSET:
            field_dict["backend"] = backend
        if installed_backend is not UNSET:
            field_dict["installed_backend"] = installed_backend
        if backup is not UNSET:
            field_dict["backup"] = backup
        if download is not UNSET:
            field_dict["download"] = download

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.managed_runtime_status_backup_type_0 import (
            ManagedRuntimeStatusBackupType0,  # noqa: PLC0415
        )
        from ..models.managed_runtime_status_download_type_0 import (
            ManagedRuntimeStatusDownloadType0,  # noqa: PLC0415
        )

        d = dict(src_dict)
        auto_update = d.pop("auto_update")

        def _parse_installed_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        installed_version = _parse_installed_version(d.pop("installed_version", UNSET))

        def _parse_previous_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        previous_version = _parse_previous_version(d.pop("previous_version", UNSET))

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        def _parse_installed_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        installed_backend = _parse_installed_backend(d.pop("installed_backend", UNSET))

        def _parse_backup(data: object) -> ManagedRuntimeStatusBackupType0 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                backup_type_0 = ManagedRuntimeStatusBackupType0.from_dict(data)

                return backup_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ManagedRuntimeStatusBackupType0 | None | Unset, data)

        backup = _parse_backup(d.pop("backup", UNSET))

        def _parse_download(data: object) -> ManagedRuntimeStatusDownloadType0 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                download_type_0 = ManagedRuntimeStatusDownloadType0.from_dict(data)

                return download_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ManagedRuntimeStatusDownloadType0 | None | Unset, data)

        download = _parse_download(d.pop("download", UNSET))

        managed_runtime_status = cls(
            auto_update=auto_update,
            installed_version=installed_version,
            previous_version=previous_version,
            backend=backend,
            installed_backend=installed_backend,
            backup=backup,
            download=download,
        )

        managed_runtime_status.additional_properties = d
        return managed_runtime_status

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
