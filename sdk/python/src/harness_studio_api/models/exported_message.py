from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.exported_variant import ExportedVariant
    from ..models.message_attachment import MessageAttachment


T = TypeVar("T", bound="ExportedMessage")


@_attrs_define
class ExportedMessage:
    """
    Attributes:
        id (str):
        role (str):
        variants (list[ExportedVariant]):
        parent_id (None | str | Unset):
        parent_variant_id (None | str | Unset):
        created_at (int | Unset):
        active_variant_id (None | str | Unset):
        attachments (list[MessageAttachment] | Unset):
    """

    id: str
    role: str
    variants: list[ExportedVariant]
    parent_id: None | str | Unset = UNSET
    parent_variant_id: None | str | Unset = UNSET
    created_at: int | Unset = UNSET
    active_variant_id: None | str | Unset = UNSET
    attachments: list[MessageAttachment] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        role = self.role

        variants = []
        for variants_item_data in self.variants:
            variants_item = variants_item_data.to_dict()
            variants.append(variants_item)

        parent_id: None | str | Unset
        if isinstance(self.parent_id, Unset):
            parent_id = UNSET
        else:
            parent_id = self.parent_id

        parent_variant_id: None | str | Unset
        if isinstance(self.parent_variant_id, Unset):
            parent_variant_id = UNSET
        else:
            parent_variant_id = self.parent_variant_id

        created_at = self.created_at

        active_variant_id: None | str | Unset
        if isinstance(self.active_variant_id, Unset):
            active_variant_id = UNSET
        else:
            active_variant_id = self.active_variant_id

        attachments: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.attachments, Unset):
            attachments = []
            for attachments_item_data in self.attachments:
                attachments_item = attachments_item_data.to_dict()
                attachments.append(attachments_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "role": role,
                "variants": variants,
            }
        )
        if parent_id is not UNSET:
            field_dict["parent_id"] = parent_id
        if parent_variant_id is not UNSET:
            field_dict["parent_variant_id"] = parent_variant_id
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if active_variant_id is not UNSET:
            field_dict["active_variant_id"] = active_variant_id
        if attachments is not UNSET:
            field_dict["attachments"] = attachments

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.exported_variant import ExportedVariant  # noqa: PLC0415
        from ..models.message_attachment import MessageAttachment  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        role = d.pop("role")

        variants = []
        _variants = d.pop("variants")
        for variants_item_data in _variants:
            variants_item = ExportedVariant.from_dict(variants_item_data)

            variants.append(variants_item)

        def _parse_parent_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        parent_id = _parse_parent_id(d.pop("parent_id", UNSET))

        def _parse_parent_variant_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        parent_variant_id = _parse_parent_variant_id(d.pop("parent_variant_id", UNSET))

        created_at = d.pop("created_at", UNSET)

        def _parse_active_variant_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        active_variant_id = _parse_active_variant_id(d.pop("active_variant_id", UNSET))

        _attachments = d.pop("attachments", UNSET)
        attachments: list[MessageAttachment] | Unset = UNSET
        if _attachments is not UNSET:
            attachments = []
            for attachments_item_data in _attachments:
                attachments_item = MessageAttachment.from_dict(attachments_item_data)

                attachments.append(attachments_item)

        exported_message = cls(
            id=id,
            role=role,
            variants=variants,
            parent_id=parent_id,
            parent_variant_id=parent_variant_id,
            created_at=created_at,
            active_variant_id=active_variant_id,
            attachments=attachments,
        )

        exported_message.additional_properties = d
        return exported_message

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
