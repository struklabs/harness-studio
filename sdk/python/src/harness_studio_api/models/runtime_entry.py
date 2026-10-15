from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.runtime_trust import RuntimeTrust, check_runtime_trust
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.file_digest import FileDigest
    from ..models.runtime_source_type_0 import RuntimeSourceType0
    from ..models.runtime_source_type_1 import RuntimeSourceType1


T = TypeVar("T", bound="RuntimeEntry")


@_attrs_define
class RuntimeEntry:
    """An extra runtime.

    Attributes:
        id (str): `[a-z0-9-]`, unique; used in definitions/profiles (`runtime` setting).
        name (str):
        source (RuntimeSourceType0 | RuntimeSourceType1): Where a runtime came from.
        dir_ (str): Folder that holds `llama-server`.
        trust (RuntimeTrust): How far the files of a runtime can be trusted.
        family (str | Unset): Runtime family (`llama.cpp`, later `whisper.cpp`); entries written before families
            existed are llama.cpp.
        version (None | str | Unset): First line of `llama-server --version`.
        build (int | None | Unset):
        commit (None | str | Unset):
        backend (None | str | Unset): ggml backend of the build (`vulkan`, `rocm`, `cuda`, `cpu`...).
        files (list[FileDigest] | Unset): Hashes of the executable files (checked at launch by size + mtime, fully by
            verify).
        quant_types (list[str] | Unset): Quant names its `llama-quantize --help` lists (empty = not detected).
        added_at (int | Unset):
        verified_at (int | Unset):
    """

    id: str
    name: str
    source: RuntimeSourceType0 | RuntimeSourceType1
    dir_: str
    trust: RuntimeTrust
    family: str | Unset = UNSET
    version: None | str | Unset = UNSET
    build: int | None | Unset = UNSET
    commit: None | str | Unset = UNSET
    backend: None | str | Unset = UNSET
    files: list[FileDigest] | Unset = UNSET
    quant_types: list[str] | Unset = UNSET
    added_at: int | Unset = UNSET
    verified_at: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.runtime_source_type_0 import RuntimeSourceType0  # noqa: PLC0415

        id = self.id

        name = self.name

        source: dict[str, Any]
        if isinstance(self.source, RuntimeSourceType0):
            source = self.source.to_dict()
        else:
            source = self.source.to_dict()

        dir_ = self.dir_

        trust: str = self.trust

        family = self.family

        version: None | str | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        else:
            version = self.version

        build: int | None | Unset
        if isinstance(self.build, Unset):
            build = UNSET
        else:
            build = self.build

        commit: None | str | Unset
        if isinstance(self.commit, Unset):
            commit = UNSET
        else:
            commit = self.commit

        backend: None | str | Unset
        if isinstance(self.backend, Unset):
            backend = UNSET
        else:
            backend = self.backend

        files: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.files, Unset):
            files = []
            for files_item_data in self.files:
                files_item = files_item_data.to_dict()
                files.append(files_item)

        quant_types: list[str] | Unset = UNSET
        if not isinstance(self.quant_types, Unset):
            quant_types = self.quant_types

        added_at = self.added_at

        verified_at = self.verified_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "source": source,
                "dir": dir_,
                "trust": trust,
            }
        )
        if family is not UNSET:
            field_dict["family"] = family
        if version is not UNSET:
            field_dict["version"] = version
        if build is not UNSET:
            field_dict["build"] = build
        if commit is not UNSET:
            field_dict["commit"] = commit
        if backend is not UNSET:
            field_dict["backend"] = backend
        if files is not UNSET:
            field_dict["files"] = files
        if quant_types is not UNSET:
            field_dict["quant_types"] = quant_types
        if added_at is not UNSET:
            field_dict["added_at"] = added_at
        if verified_at is not UNSET:
            field_dict["verified_at"] = verified_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.file_digest import FileDigest  # noqa: PLC0415
        from ..models.runtime_source_type_0 import RuntimeSourceType0  # noqa: PLC0415
        from ..models.runtime_source_type_1 import RuntimeSourceType1  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        def _parse_source(data: object) -> RuntimeSourceType0 | RuntimeSourceType1:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_runtime_source_type_0 = RuntimeSourceType0.from_dict(data)

                return componentsschemas_runtime_source_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_runtime_source_type_1 = RuntimeSourceType1.from_dict(data)

            return componentsschemas_runtime_source_type_1

        source = _parse_source(d.pop("source"))

        dir_ = d.pop("dir")

        trust = check_runtime_trust(d.pop("trust"))

        family = d.pop("family", UNSET)

        def _parse_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        version = _parse_version(d.pop("version", UNSET))

        def _parse_build(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        build = _parse_build(d.pop("build", UNSET))

        def _parse_commit(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        commit = _parse_commit(d.pop("commit", UNSET))

        def _parse_backend(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        backend = _parse_backend(d.pop("backend", UNSET))

        _files = d.pop("files", UNSET)
        files: list[FileDigest] | Unset = UNSET
        if _files is not UNSET:
            files = []
            for files_item_data in _files:
                files_item = FileDigest.from_dict(files_item_data)

                files.append(files_item)

        quant_types = cast(list[str], d.pop("quant_types", UNSET))

        added_at = d.pop("added_at", UNSET)

        verified_at = d.pop("verified_at", UNSET)

        runtime_entry = cls(
            id=id,
            name=name,
            source=source,
            dir_=dir_,
            trust=trust,
            family=family,
            version=version,
            build=build,
            commit=commit,
            backend=backend,
            files=files,
            quant_types=quant_types,
            added_at=added_at,
            verified_at=verified_at,
        )

        runtime_entry.additional_properties = d
        return runtime_entry

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
