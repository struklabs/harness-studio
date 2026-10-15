from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.library_entry import LibraryEntry
    from ..models.model_definition import ModelDefinition


T = TypeVar("T", bound="ModelsResponse")


@_attrs_define
class ModelsResponse:
    """The library: every local model (all kinds: LLM, embedding, reranker, projector, adapter)
    plus the model definitions built on them.

        Attributes:
            entries (list[LibraryEntry]):
            definitions (list[ModelDefinition]):
            state_error (None | str | Unset): `state.db` unavailable: favorites / last use missing.
    """

    entries: list[LibraryEntry]
    definitions: list[ModelDefinition]
    state_error: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        entries = []
        for entries_item_data in self.entries:
            entries_item = entries_item_data.to_dict()
            entries.append(entries_item)

        definitions = []
        for definitions_item_data in self.definitions:
            definitions_item = definitions_item_data.to_dict()
            definitions.append(definitions_item)

        state_error: None | str | Unset
        if isinstance(self.state_error, Unset):
            state_error = UNSET
        else:
            state_error = self.state_error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "entries": entries,
                "definitions": definitions,
            }
        )
        if state_error is not UNSET:
            field_dict["state_error"] = state_error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.library_entry import LibraryEntry  # noqa: PLC0415
        from ..models.model_definition import ModelDefinition  # noqa: PLC0415

        d = dict(src_dict)
        entries = []
        _entries = d.pop("entries")
        for entries_item_data in _entries:
            entries_item = LibraryEntry.from_dict(entries_item_data)

            entries.append(entries_item)

        definitions = []
        _definitions = d.pop("definitions")
        for definitions_item_data in _definitions:
            definitions_item = ModelDefinition.from_dict(definitions_item_data)

            definitions.append(definitions_item)

        def _parse_state_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        state_error = _parse_state_error(d.pop("state_error", UNSET))

        models_response = cls(
            entries=entries,
            definitions=definitions,
            state_error=state_error,
        )

        models_response.additional_properties = d
        return models_response

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
