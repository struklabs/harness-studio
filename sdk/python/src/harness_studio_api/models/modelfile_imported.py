from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.model_definition import ModelDefinition
    from ..models.modelfile_warning import ModelfileWarning


T = TypeVar("T", bound="ModelfileImported")


@_attrs_define
class ModelfileImported:
    """A saved Modelfile import.

    Attributes:
        definition (ModelDefinition): A logical model: what clients ask for by `alias`.
        warnings (list[ModelfileWarning]): What could not be carried over.
    """

    definition: ModelDefinition
    warnings: list[ModelfileWarning]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        definition = self.definition.to_dict()

        warnings = []
        for warnings_item_data in self.warnings:
            warnings_item = warnings_item_data.to_dict()
            warnings.append(warnings_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "definition": definition,
                "warnings": warnings,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.model_definition import ModelDefinition  # noqa: PLC0415
        from ..models.modelfile_warning import ModelfileWarning  # noqa: PLC0415

        d = dict(src_dict)
        definition = ModelDefinition.from_dict(d.pop("definition"))

        warnings = []
        _warnings = d.pop("warnings")
        for warnings_item_data in _warnings:
            warnings_item = ModelfileWarning.from_dict(warnings_item_data)

            warnings.append(warnings_item)

        modelfile_imported = cls(
            definition=definition,
            warnings=warnings,
        )

        modelfile_imported.additional_properties = d
        return modelfile_imported

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
