from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RagDocument")


@_attrs_define
class RagDocument:
    """A document attached to a conversation (without its text).

    Attributes:
        id (str):
        conversation_id (str):
        name (str):
        kind (str): `pdf | docx | markdown | code | text`.
        size (int):
        sha256 (str):
        chars (int):
        tokens (int): Estimated tokens of the whole text (`search::estimate_tokens`).
        chunks (int):
        embedded (int): Chunks that have an embedding of `embed_model`.
        status (str): `pending | extracting | embedding | ready | failed | cancelled`.
        mode (str): `auto | full | retrieval`.
        created_at (int):
        updated_at (int):
        source_path (None | str | Unset):
        pages (int | None | Unset):
        error (None | str | Unset): Keyed error of a failed document.
        embed_model (None | str | Unset):
        embed_dims (int | None | Unset):
    """

    id: str
    conversation_id: str
    name: str
    kind: str
    size: int
    sha256: str
    chars: int
    tokens: int
    chunks: int
    embedded: int
    status: str
    mode: str
    created_at: int
    updated_at: int
    source_path: None | str | Unset = UNSET
    pages: int | None | Unset = UNSET
    error: None | str | Unset = UNSET
    embed_model: None | str | Unset = UNSET
    embed_dims: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        conversation_id = self.conversation_id

        name = self.name

        kind = self.kind

        size = self.size

        sha256 = self.sha256

        chars = self.chars

        tokens = self.tokens

        chunks = self.chunks

        embedded = self.embedded

        status = self.status

        mode = self.mode

        created_at = self.created_at

        updated_at = self.updated_at

        source_path: None | str | Unset
        if isinstance(self.source_path, Unset):
            source_path = UNSET
        else:
            source_path = self.source_path

        pages: int | None | Unset
        if isinstance(self.pages, Unset):
            pages = UNSET
        else:
            pages = self.pages

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

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

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "conversation_id": conversation_id,
                "name": name,
                "kind": kind,
                "size": size,
                "sha256": sha256,
                "chars": chars,
                "tokens": tokens,
                "chunks": chunks,
                "embedded": embedded,
                "status": status,
                "mode": mode,
                "created_at": created_at,
                "updated_at": updated_at,
            }
        )
        if source_path is not UNSET:
            field_dict["source_path"] = source_path
        if pages is not UNSET:
            field_dict["pages"] = pages
        if error is not UNSET:
            field_dict["error"] = error
        if embed_model is not UNSET:
            field_dict["embed_model"] = embed_model
        if embed_dims is not UNSET:
            field_dict["embed_dims"] = embed_dims

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        conversation_id = d.pop("conversation_id")

        name = d.pop("name")

        kind = d.pop("kind")

        size = d.pop("size")

        sha256 = d.pop("sha256")

        chars = d.pop("chars")

        tokens = d.pop("tokens")

        chunks = d.pop("chunks")

        embedded = d.pop("embedded")

        status = d.pop("status")

        mode = d.pop("mode")

        created_at = d.pop("created_at")

        updated_at = d.pop("updated_at")

        def _parse_source_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source_path = _parse_source_path(d.pop("source_path", UNSET))

        def _parse_pages(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        pages = _parse_pages(d.pop("pages", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

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

        rag_document = cls(
            id=id,
            conversation_id=conversation_id,
            name=name,
            kind=kind,
            size=size,
            sha256=sha256,
            chars=chars,
            tokens=tokens,
            chunks=chunks,
            embedded=embedded,
            status=status,
            mode=mode,
            created_at=created_at,
            updated_at=updated_at,
            source_path=source_path,
            pages=pages,
            error=error,
            embed_model=embed_model,
            embed_dims=embed_dims,
        )

        rag_document.additional_properties = d
        return rag_document

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
