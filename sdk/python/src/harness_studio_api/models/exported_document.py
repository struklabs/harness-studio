from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.exported_chunk import ExportedChunk


T = TypeVar("T", bound="ExportedDocument")


@_attrs_define
class ExportedDocument:
    """A document of the conversation. A file export carries the text and chunks but no vectors:
    an imported document is searchable by keywords at once and gets embeddings again when the
    user indexes it with an embedding model. Duplicating or forking a conversation copies the
    vectors as well (the `embedding` fields never reach a file).

        Attributes:
            id (str | Unset):  Default: ''.
            name (str | Unset):  Default: ''.
            kind (str | Unset):  Default: ''.
            size (int | Unset):  Default: 0.
            sha256 (str | Unset):  Default: ''.
            pages (int | None | Unset):
            content (str | Unset):  Default: ''.
            mode (str | Unset): `auto | full | retrieval`. Default: ''.
            embed_model (None | str | Unset):
            embed_dims (int | None | Unset):
            created_at (int | Unset):  Default: 0.
            chunks (list[ExportedChunk] | Unset):
    """

    id: str | Unset = ""
    name: str | Unset = ""
    kind: str | Unset = ""
    size: int | Unset = 0
    sha256: str | Unset = ""
    pages: int | None | Unset = UNSET
    content: str | Unset = ""
    mode: str | Unset = ""
    embed_model: None | str | Unset = UNSET
    embed_dims: int | None | Unset = UNSET
    created_at: int | Unset = 0
    chunks: list[ExportedChunk] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        kind = self.kind

        size = self.size

        sha256 = self.sha256

        pages: int | None | Unset
        if isinstance(self.pages, Unset):
            pages = UNSET
        else:
            pages = self.pages

        content = self.content

        mode = self.mode

        embed_model: None | str | Unset
        if isinstance(self.embed_model, Unset):
            embed_model = UNSET
        else:
            embed_model = self.embed_model

        embed_dims: int | None | Unset
        if isinstance(self.embed_dims, Unset):
            embed_dims = UNSET
        else:
            embed_dims = self.embed_dims

        created_at = self.created_at

        chunks: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.chunks, Unset):
            chunks = []
            for chunks_item_data in self.chunks:
                chunks_item = chunks_item_data.to_dict()
                chunks.append(chunks_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if id is not UNSET:
            field_dict["id"] = id
        if name is not UNSET:
            field_dict["name"] = name
        if kind is not UNSET:
            field_dict["kind"] = kind
        if size is not UNSET:
            field_dict["size"] = size
        if sha256 is not UNSET:
            field_dict["sha256"] = sha256
        if pages is not UNSET:
            field_dict["pages"] = pages
        if content is not UNSET:
            field_dict["content"] = content
        if mode is not UNSET:
            field_dict["mode"] = mode
        if embed_model is not UNSET:
            field_dict["embed_model"] = embed_model
        if embed_dims is not UNSET:
            field_dict["embed_dims"] = embed_dims
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if chunks is not UNSET:
            field_dict["chunks"] = chunks

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.exported_chunk import ExportedChunk  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id", UNSET)

        name = d.pop("name", UNSET)

        kind = d.pop("kind", UNSET)

        size = d.pop("size", UNSET)

        sha256 = d.pop("sha256", UNSET)

        def _parse_pages(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        pages = _parse_pages(d.pop("pages", UNSET))

        content = d.pop("content", UNSET)

        mode = d.pop("mode", UNSET)

        def _parse_embed_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        embed_model = _parse_embed_model(d.pop("embed_model", UNSET))

        def _parse_embed_dims(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        embed_dims = _parse_embed_dims(d.pop("embed_dims", UNSET))

        created_at = d.pop("created_at", UNSET)

        _chunks = d.pop("chunks", UNSET)
        chunks: list[ExportedChunk] | Unset = UNSET
        if _chunks is not UNSET:
            chunks = []
            for chunks_item_data in _chunks:
                chunks_item = ExportedChunk.from_dict(chunks_item_data)

                chunks.append(chunks_item)

        exported_document = cls(
            id=id,
            name=name,
            kind=kind,
            size=size,
            sha256=sha256,
            pages=pages,
            content=content,
            mode=mode,
            embed_model=embed_model,
            embed_dims=embed_dims,
            created_at=created_at,
            chunks=chunks,
        )

        exported_document.additional_properties = d
        return exported_document

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
