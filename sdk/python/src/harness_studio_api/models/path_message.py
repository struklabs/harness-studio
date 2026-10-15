from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.message_variant import MessageVariant


T = TypeVar("T", bound="PathMessage")


@_attrs_define
class PathMessage:
    """A message on the current path, with its shown variant and navigation info.

    Attributes:
        id (str):
        role (str):
        created_at (int):
        variant (MessageVariant):
        variant_ids (list[str]): All variants of this message, oldest first.
        variant_index (int):
        sibling_ids (list[str]): Messages sharing this message's parent (and parent variant), oldest first.
        sibling_index (int):
        parent_id (None | str | Unset):
        parent_variant_id (None | str | Unset):
        compare (list[MessageVariant] | Unset): Split view: every variant of the shown variant's comparison group (sides
            `a`, `b`),
            in side order. Empty when the shown variant is not part of a comparison.
        compare_groups (list[str] | Unset): Split view: one variant id per comparison group of this message (the
            side-`a` variant),
            oldest group first. More than one group means the turn was regenerated; the shown
            group is the one `compare` belongs to. Empty when the message has no comparison.
    """

    id: str
    role: str
    created_at: int
    variant: MessageVariant
    variant_ids: list[str]
    variant_index: int
    sibling_ids: list[str]
    sibling_index: int
    parent_id: None | str | Unset = UNSET
    parent_variant_id: None | str | Unset = UNSET
    compare: list[MessageVariant] | Unset = UNSET
    compare_groups: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        role = self.role

        created_at = self.created_at

        variant = self.variant.to_dict()

        variant_ids = self.variant_ids

        variant_index = self.variant_index

        sibling_ids = self.sibling_ids

        sibling_index = self.sibling_index

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

        compare: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.compare, Unset):
            compare = []
            for compare_item_data in self.compare:
                compare_item = compare_item_data.to_dict()
                compare.append(compare_item)

        compare_groups: list[str] | Unset = UNSET
        if not isinstance(self.compare_groups, Unset):
            compare_groups = self.compare_groups

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "role": role,
                "created_at": created_at,
                "variant": variant,
                "variant_ids": variant_ids,
                "variant_index": variant_index,
                "sibling_ids": sibling_ids,
                "sibling_index": sibling_index,
            }
        )
        if parent_id is not UNSET:
            field_dict["parent_id"] = parent_id
        if parent_variant_id is not UNSET:
            field_dict["parent_variant_id"] = parent_variant_id
        if compare is not UNSET:
            field_dict["compare"] = compare
        if compare_groups is not UNSET:
            field_dict["compare_groups"] = compare_groups

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.message_variant import MessageVariant  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        role = d.pop("role")

        created_at = d.pop("created_at")

        variant = MessageVariant.from_dict(d.pop("variant"))

        variant_ids = cast(list[str], d.pop("variant_ids"))

        variant_index = d.pop("variant_index")

        sibling_ids = cast(list[str], d.pop("sibling_ids"))

        sibling_index = d.pop("sibling_index")

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

        _compare = d.pop("compare", UNSET)
        compare: list[MessageVariant] | Unset = UNSET
        if _compare is not UNSET:
            compare = []
            for compare_item_data in _compare:
                compare_item = MessageVariant.from_dict(compare_item_data)

                compare.append(compare_item)

        compare_groups = cast(list[str], d.pop("compare_groups", UNSET))

        path_message = cls(
            id=id,
            role=role,
            created_at=created_at,
            variant=variant,
            variant_ids=variant_ids,
            variant_index=variant_index,
            sibling_ids=sibling_ids,
            sibling_index=sibling_index,
            parent_id=parent_id,
            parent_variant_id=parent_variant_id,
            compare=compare,
            compare_groups=compare_groups,
        )

        path_message.additional_properties = d
        return path_message

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
