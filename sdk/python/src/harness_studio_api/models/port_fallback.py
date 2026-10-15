from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.port_holder import PortHolder


T = TypeVar("T", bound="PortFallback")


@_attrs_define
class PortFallback:
    """A listener that could not use its configured port.

    Attributes:
        configured (int): The configured port, held by another program.
        holder (None | PortHolder | Unset):
    """

    configured: int
    holder: None | PortHolder | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.port_holder import PortHolder  # noqa: PLC0415

        configured = self.configured

        holder: dict[str, Any] | None | Unset
        if isinstance(self.holder, Unset):
            holder = UNSET
        elif isinstance(self.holder, PortHolder):
            holder = self.holder.to_dict()
        else:
            holder = self.holder

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "configured": configured,
            }
        )
        if holder is not UNSET:
            field_dict["holder"] = holder

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.port_holder import PortHolder  # noqa: PLC0415

        d = dict(src_dict)
        configured = d.pop("configured")

        def _parse_holder(data: object) -> None | PortHolder | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                holder_type_1 = PortHolder.from_dict(data)

                return holder_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | PortHolder | Unset, data)

        holder = _parse_holder(d.pop("holder", UNSET))

        port_fallback = cls(
            configured=configured,
            holder=holder,
        )

        port_fallback.additional_properties = d
        return port_fallback

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
