from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.model_kind import ModelKind, check_model_kind
from ..models.model_origin import ModelOrigin, check_model_origin
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.gguf_metadata import GgufMetadata


T = TypeVar("T", bound="LocalModel")


@_attrs_define
class LocalModel:
    """
    Attributes:
        id (str):
        name (str):
        filename (str):
        path (str):
        size_bytes (int):
        quantization (str):
        is_split (bool):
        is_ready (bool):
        shard_paths (list[str]):
        root_dir (str):
        architecture (None | str | Unset):
        context_length (int | None | Unset):
        total_shards (int | None | Unset):
        present_shards (int | None | Unset):
        gguf (GgufMetadata | None | Unset):
        source (None | str | Unset): "ollama" | "lmstudio" for models linked in place from another app
            (not inside a library directory; never moved or deleted by us).
        mmproj_path (None | str | Unset): Vision projector that belongs to this model (known for imported models).
        kind (ModelKind | Unset): What a GGUF file is. Decided from its header by [`detect_kind`].
        origin (ModelOrigin | Unset): Where a scanned model lives.
        task (None | str | Unset): Hugging Face pipeline task of a [`crate::domain::ModelKind::Pipeline`] model
            (`text-classification`, `token-classification`...).
    """

    id: str
    name: str
    filename: str
    path: str
    size_bytes: int
    quantization: str
    is_split: bool
    is_ready: bool
    shard_paths: list[str]
    root_dir: str
    architecture: None | str | Unset = UNSET
    context_length: int | None | Unset = UNSET
    total_shards: int | None | Unset = UNSET
    present_shards: int | None | Unset = UNSET
    gguf: GgufMetadata | None | Unset = UNSET
    source: None | str | Unset = UNSET
    mmproj_path: None | str | Unset = UNSET
    kind: ModelKind | Unset = UNSET
    origin: ModelOrigin | Unset = UNSET
    task: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.gguf_metadata import GgufMetadata  # noqa: PLC0415

        id = self.id

        name = self.name

        filename = self.filename

        path = self.path

        size_bytes = self.size_bytes

        quantization = self.quantization

        is_split = self.is_split

        is_ready = self.is_ready

        shard_paths = self.shard_paths

        root_dir = self.root_dir

        architecture: None | str | Unset
        if isinstance(self.architecture, Unset):
            architecture = UNSET
        else:
            architecture = self.architecture

        context_length: int | None | Unset
        if isinstance(self.context_length, Unset):
            context_length = UNSET
        else:
            context_length = self.context_length

        total_shards: int | None | Unset
        if isinstance(self.total_shards, Unset):
            total_shards = UNSET
        else:
            total_shards = self.total_shards

        present_shards: int | None | Unset
        if isinstance(self.present_shards, Unset):
            present_shards = UNSET
        else:
            present_shards = self.present_shards

        gguf: dict[str, Any] | None | Unset
        if isinstance(self.gguf, Unset):
            gguf = UNSET
        elif isinstance(self.gguf, GgufMetadata):
            gguf = self.gguf.to_dict()
        else:
            gguf = self.gguf

        source: None | str | Unset
        if isinstance(self.source, Unset):
            source = UNSET
        else:
            source = self.source

        mmproj_path: None | str | Unset
        if isinstance(self.mmproj_path, Unset):
            mmproj_path = UNSET
        else:
            mmproj_path = self.mmproj_path

        kind: str | Unset = UNSET
        if not isinstance(self.kind, Unset):
            kind = self.kind

        origin: str | Unset = UNSET
        if not isinstance(self.origin, Unset):
            origin = self.origin

        task: None | str | Unset
        if isinstance(self.task, Unset):
            task = UNSET
        else:
            task = self.task

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "filename": filename,
                "path": path,
                "size_bytes": size_bytes,
                "quantization": quantization,
                "is_split": is_split,
                "is_ready": is_ready,
                "shard_paths": shard_paths,
                "root_dir": root_dir,
            }
        )
        if architecture is not UNSET:
            field_dict["architecture"] = architecture
        if context_length is not UNSET:
            field_dict["context_length"] = context_length
        if total_shards is not UNSET:
            field_dict["total_shards"] = total_shards
        if present_shards is not UNSET:
            field_dict["present_shards"] = present_shards
        if gguf is not UNSET:
            field_dict["gguf"] = gguf
        if source is not UNSET:
            field_dict["source"] = source
        if mmproj_path is not UNSET:
            field_dict["mmproj_path"] = mmproj_path
        if kind is not UNSET:
            field_dict["kind"] = kind
        if origin is not UNSET:
            field_dict["origin"] = origin
        if task is not UNSET:
            field_dict["task"] = task

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gguf_metadata import GgufMetadata  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        filename = d.pop("filename")

        path = d.pop("path")

        size_bytes = d.pop("size_bytes")

        quantization = d.pop("quantization")

        is_split = d.pop("is_split")

        is_ready = d.pop("is_ready")

        shard_paths = cast(list[str], d.pop("shard_paths"))

        root_dir = d.pop("root_dir")

        def _parse_architecture(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        architecture = _parse_architecture(d.pop("architecture", UNSET))

        def _parse_context_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        context_length = _parse_context_length(d.pop("context_length", UNSET))

        def _parse_total_shards(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_shards = _parse_total_shards(d.pop("total_shards", UNSET))

        def _parse_present_shards(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        present_shards = _parse_present_shards(d.pop("present_shards", UNSET))

        def _parse_gguf(data: object) -> GgufMetadata | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                gguf_type_1 = GgufMetadata.from_dict(data)

                return gguf_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(GgufMetadata | None | Unset, data)

        gguf = _parse_gguf(d.pop("gguf", UNSET))

        def _parse_source(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source = _parse_source(d.pop("source", UNSET))

        def _parse_mmproj_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mmproj_path = _parse_mmproj_path(d.pop("mmproj_path", UNSET))

        _kind = d.pop("kind", UNSET)
        kind: ModelKind | Unset
        if isinstance(_kind, Unset):
            kind = UNSET
        else:
            kind = check_model_kind(_kind)

        _origin = d.pop("origin", UNSET)
        origin: ModelOrigin | Unset
        if isinstance(_origin, Unset):
            origin = UNSET
        else:
            origin = check_model_origin(_origin)

        def _parse_task(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        task = _parse_task(d.pop("task", UNSET))

        local_model = cls(
            id=id,
            name=name,
            filename=filename,
            path=path,
            size_bytes=size_bytes,
            quantization=quantization,
            is_split=is_split,
            is_ready=is_ready,
            shard_paths=shard_paths,
            root_dir=root_dir,
            architecture=architecture,
            context_length=context_length,
            total_shards=total_shards,
            present_shards=present_shards,
            gguf=gguf,
            source=source,
            mmproj_path=mmproj_path,
            kind=kind,
            origin=origin,
            task=task,
        )

        local_model.additional_properties = d
        return local_model

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
