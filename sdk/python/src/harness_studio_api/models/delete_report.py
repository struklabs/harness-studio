from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="DeleteReport")


@_attrs_define
class DeleteReport:
    """Outcome of [`delete_model`].

    Attributes:
        files (list[str]):
        definitions_deleted (list[str]): Definitions based on the file that were deleted with it (on request).
        definition_errors (list[str]): Definitions that could not be deleted (e.g. a profile serves them).
    """

    files: list[str]
    definitions_deleted: list[str]
    definition_errors: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        files = self.files

        definitions_deleted = self.definitions_deleted

        definition_errors = self.definition_errors

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "files": files,
                "definitions_deleted": definitions_deleted,
                "definition_errors": definition_errors,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        files = cast(list[str], d.pop("files"))

        definitions_deleted = cast(list[str], d.pop("definitions_deleted"))

        definition_errors = cast(list[str], d.pop("definition_errors"))

        delete_report = cls(
            files=files,
            definitions_deleted=definitions_deleted,
            definition_errors=definition_errors,
        )

        delete_report.additional_properties = d
        return delete_report

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
