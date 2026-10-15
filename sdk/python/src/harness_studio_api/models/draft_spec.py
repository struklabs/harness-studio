from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.b_tree_map import BTreeMap


T = TypeVar("T", bound="DraftSpec")


@_attrs_define
class DraftSpec:
    """Speculative decoding with a draft model.

    Attributes:
        model (str):
        params (BTreeMap | Unset):
    """

    model: str
    params: BTreeMap | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model = self.model

        params: dict[str, Any] | Unset = UNSET
        if not isinstance(self.params, Unset):
            params = self.params.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "model": model,
            }
        )
        if params is not UNSET:
            field_dict["params"] = params

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.b_tree_map import BTreeMap  # noqa: PLC0415

        d = dict(src_dict)
        model = d.pop("model")

        _params = d.pop("params", UNSET)
        params: BTreeMap | Unset
        if isinstance(_params, Unset):
            params = UNSET
        else:
            params = BTreeMap.from_dict(_params)

        draft_spec = cls(
            model=model,
            params=params,
        )

        draft_spec.additional_properties = d
        return draft_spec

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
