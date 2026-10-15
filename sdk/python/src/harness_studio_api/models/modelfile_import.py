from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.definition_import import DefinitionImport
    from ..models.modelfile_mapped import ModelfileMapped
    from ..models.modelfile_warning import ModelfileWarning


T = TypeVar("T", bound="ModelfileImport")


@_attrs_define
class ModelfileImport:
    """What a Modelfile import found (nothing is saved yet).

    Attributes:
        import_ (DefinitionImport): What an import found (nothing is saved yet).
        mapped (list[ModelfileMapped]):
        warnings (list[ModelfileWarning]):
        from_ (str): `FROM` as written.
    """

    import_: DefinitionImport
    mapped: list[ModelfileMapped]
    warnings: list[ModelfileWarning]
    from_: str
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        import_ = self.import_.to_dict()

        mapped = []
        for mapped_item_data in self.mapped:
            mapped_item = mapped_item_data.to_dict()
            mapped.append(mapped_item)

        warnings = []
        for warnings_item_data in self.warnings:
            warnings_item = warnings_item_data.to_dict()
            warnings.append(warnings_item)

        from_ = self.from_

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "import": import_,
                "mapped": mapped,
                "warnings": warnings,
                "from": from_,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.definition_import import DefinitionImport  # noqa: PLC0415
        from ..models.modelfile_mapped import ModelfileMapped  # noqa: PLC0415
        from ..models.modelfile_warning import ModelfileWarning  # noqa: PLC0415

        d = dict(src_dict)
        import_ = DefinitionImport.from_dict(d.pop("import"))

        mapped = []
        _mapped = d.pop("mapped")
        for mapped_item_data in _mapped:
            mapped_item = ModelfileMapped.from_dict(mapped_item_data)

            mapped.append(mapped_item)

        warnings = []
        _warnings = d.pop("warnings")
        for warnings_item_data in _warnings:
            warnings_item = ModelfileWarning.from_dict(warnings_item_data)

            warnings.append(warnings_item)

        from_ = d.pop("from")

        modelfile_import = cls(
            import_=import_,
            mapped=mapped,
            warnings=warnings,
            from_=from_,
        )

        modelfile_import.additional_properties = d
        return modelfile_import

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
