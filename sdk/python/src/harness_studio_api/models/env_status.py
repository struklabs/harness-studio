from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.env_progress import EnvProgress
    from ..models.manifest import Manifest


T = TypeVar("T", bound="EnvStatus")


@_attrs_define
class EnvStatus:
    """What Settings shows.

    Attributes:
        supported (bool): This platform can build the environment (uv has a build for it).
        installed (bool): A complete environment exists (it may be for another llama.cpp tag).
        ready (bool): It matches the managed runtime's tag, so a conversion can start at once.
        size_bytes (int):
        dir_ (str):
        estimated_size_bytes (int): Rough size a first build adds.
        runtime_tag (None | str | Unset): Tag of the managed llama.cpp runtime (`None` when it is not installed).
        manifest (Manifest | None | Unset):
        progress (EnvProgress | None | Unset):
    """

    supported: bool
    installed: bool
    ready: bool
    size_bytes: int
    dir_: str
    estimated_size_bytes: int
    runtime_tag: None | str | Unset = UNSET
    manifest: Manifest | None | Unset = UNSET
    progress: EnvProgress | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.env_progress import EnvProgress  # noqa: PLC0415
        from ..models.manifest import Manifest  # noqa: PLC0415

        supported = self.supported

        installed = self.installed

        ready = self.ready

        size_bytes = self.size_bytes

        dir_ = self.dir_

        estimated_size_bytes = self.estimated_size_bytes

        runtime_tag: None | str | Unset
        if isinstance(self.runtime_tag, Unset):
            runtime_tag = UNSET
        else:
            runtime_tag = self.runtime_tag

        manifest: dict[str, Any] | None | Unset
        if isinstance(self.manifest, Unset):
            manifest = UNSET
        elif isinstance(self.manifest, Manifest):
            manifest = self.manifest.to_dict()
        else:
            manifest = self.manifest

        progress: dict[str, Any] | None | Unset
        if isinstance(self.progress, Unset):
            progress = UNSET
        elif isinstance(self.progress, EnvProgress):
            progress = self.progress.to_dict()
        else:
            progress = self.progress

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "supported": supported,
                "installed": installed,
                "ready": ready,
                "size_bytes": size_bytes,
                "dir": dir_,
                "estimated_size_bytes": estimated_size_bytes,
            }
        )
        if runtime_tag is not UNSET:
            field_dict["runtime_tag"] = runtime_tag
        if manifest is not UNSET:
            field_dict["manifest"] = manifest
        if progress is not UNSET:
            field_dict["progress"] = progress

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.env_progress import EnvProgress  # noqa: PLC0415
        from ..models.manifest import Manifest  # noqa: PLC0415

        d = dict(src_dict)
        supported = d.pop("supported")

        installed = d.pop("installed")

        ready = d.pop("ready")

        size_bytes = d.pop("size_bytes")

        dir_ = d.pop("dir")

        estimated_size_bytes = d.pop("estimated_size_bytes")

        def _parse_runtime_tag(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        runtime_tag = _parse_runtime_tag(d.pop("runtime_tag", UNSET))

        def _parse_manifest(data: object) -> Manifest | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                manifest_type_1 = Manifest.from_dict(data)

                return manifest_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Manifest | None | Unset, data)

        manifest = _parse_manifest(d.pop("manifest", UNSET))

        def _parse_progress(data: object) -> EnvProgress | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                progress_type_1 = EnvProgress.from_dict(data)

                return progress_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(EnvProgress | None | Unset, data)

        progress = _parse_progress(d.pop("progress", UNSET))

        env_status = cls(
            supported=supported,
            installed=installed,
            ready=ready,
            size_bytes=size_bytes,
            dir_=dir_,
            estimated_size_bytes=estimated_size_bytes,
            runtime_tag=runtime_tag,
            manifest=manifest,
            progress=progress,
        )

        env_status.additional_properties = d
        return env_status

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
