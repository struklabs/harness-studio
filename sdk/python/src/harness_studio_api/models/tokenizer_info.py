from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="TokenizerInfo")


@_attrs_define
class TokenizerInfo:
    """Tokenizer facts of a GGUF (`tokenizer.ggml.*`), enough to tell whether two models can
    be target and draft of speculative decoding (see `library::draft`).

        Attributes:
            model (None | str | Unset): `tokenizer.ggml.model` ("gpt2", "llama", "t5", "gemma4"...).
            pre (None | str | Unset): `tokenizer.ggml.pre` (pre-tokenizer: "qwen2", "gpt-4o", "default"...).
            vocab_size (int | None | Unset): Number of entries in `tokenizer.ggml.tokens`.
            bos_id (int | None | Unset):
            eos_id (int | None | Unset):
            add_bos (bool | None | Unset):
            add_eos (bool | None | Unset):
            head_hash (None | str | Unset): SHA-256 (hex, first 16 bytes) of the token texts from id [`TOKEN_HASH_START`]
                ([`TOKEN_HASH_LEN`] of them; llama.cpp compares token texts from id 5 on). `None` for
                smaller vocabularies.
    """

    model: None | str | Unset = UNSET
    pre: None | str | Unset = UNSET
    vocab_size: int | None | Unset = UNSET
    bos_id: int | None | Unset = UNSET
    eos_id: int | None | Unset = UNSET
    add_bos: bool | None | Unset = UNSET
    add_eos: bool | None | Unset = UNSET
    head_hash: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        pre: None | str | Unset
        if isinstance(self.pre, Unset):
            pre = UNSET
        else:
            pre = self.pre

        vocab_size: int | None | Unset
        if isinstance(self.vocab_size, Unset):
            vocab_size = UNSET
        else:
            vocab_size = self.vocab_size

        bos_id: int | None | Unset
        if isinstance(self.bos_id, Unset):
            bos_id = UNSET
        else:
            bos_id = self.bos_id

        eos_id: int | None | Unset
        if isinstance(self.eos_id, Unset):
            eos_id = UNSET
        else:
            eos_id = self.eos_id

        add_bos: bool | None | Unset
        if isinstance(self.add_bos, Unset):
            add_bos = UNSET
        else:
            add_bos = self.add_bos

        add_eos: bool | None | Unset
        if isinstance(self.add_eos, Unset):
            add_eos = UNSET
        else:
            add_eos = self.add_eos

        head_hash: None | str | Unset
        if isinstance(self.head_hash, Unset):
            head_hash = UNSET
        else:
            head_hash = self.head_hash

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if model is not UNSET:
            field_dict["model"] = model
        if pre is not UNSET:
            field_dict["pre"] = pre
        if vocab_size is not UNSET:
            field_dict["vocab_size"] = vocab_size
        if bos_id is not UNSET:
            field_dict["bos_id"] = bos_id
        if eos_id is not UNSET:
            field_dict["eos_id"] = eos_id
        if add_bos is not UNSET:
            field_dict["add_bos"] = add_bos
        if add_eos is not UNSET:
            field_dict["add_eos"] = add_eos
        if head_hash is not UNSET:
            field_dict["head_hash"] = head_hash

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_pre(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        pre = _parse_pre(d.pop("pre", UNSET))

        def _parse_vocab_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vocab_size = _parse_vocab_size(d.pop("vocab_size", UNSET))

        def _parse_bos_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        bos_id = _parse_bos_id(d.pop("bos_id", UNSET))

        def _parse_eos_id(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        eos_id = _parse_eos_id(d.pop("eos_id", UNSET))

        def _parse_add_bos(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        add_bos = _parse_add_bos(d.pop("add_bos", UNSET))

        def _parse_add_eos(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        add_eos = _parse_add_eos(d.pop("add_eos", UNSET))

        def _parse_head_hash(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        head_hash = _parse_head_hash(d.pop("head_hash", UNSET))

        tokenizer_info = cls(
            model=model,
            pre=pre,
            vocab_size=vocab_size,
            bos_id=bos_id,
            eos_id=eos_id,
            add_bos=add_bos,
            add_eos=add_eos,
            head_hash=head_hash,
        )

        tokenizer_info.additional_properties = d
        return tokenizer_info

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
