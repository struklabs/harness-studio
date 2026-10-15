from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.support import Support, check_support

if TYPE_CHECKING:
    from ..models.tensor_type import TensorType


T = TypeVar("T", bound="RuntimeFit")


@_attrs_define
class RuntimeFit:
    """How well each runtime fits a model file.

    Attributes:
        runtime_id (str):
        support (Support):
        missing (list[TensorType]): Tensor types this runtime can't load.
    """

    runtime_id: str
    support: Support
    missing: list[TensorType]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        runtime_id = self.runtime_id

        support: str = self.support

        missing = []
        for missing_item_data in self.missing:
            missing_item = missing_item_data.to_dict()
            missing.append(missing_item)

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "runtime_id": runtime_id,
                "support": support,
                "missing": missing,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tensor_type import TensorType  # noqa: PLC0415

        d = dict(src_dict)
        runtime_id = d.pop("runtime_id")

        support = check_support(d.pop("support"))

        missing = []
        _missing = d.pop("missing")
        for missing_item_data in _missing:
            missing_item = TensorType.from_dict(missing_item_data)

            missing.append(missing_item)

        runtime_fit = cls(
            runtime_id=runtime_id,
            support=support,
            missing=missing,
        )

        runtime_fit.additional_properties = d
        return runtime_fit

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
