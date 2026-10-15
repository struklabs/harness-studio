from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.hf_model_variant import HfModelVariant


T = TypeVar("T", bound="PullResponse")


@_attrs_define
class PullResponse:
    """
    Attributes:
        job_id (int): Download job id (`/downloads/{id}`, `download` events).
        variant (HfModelVariant):
        companions (list[int]): Download jobs of the companion files a known image model needs (VAE, text encoders);
            empty for every other model.
    """

    job_id: int
    variant: HfModelVariant
    companions: list[int]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        job_id = self.job_id

        variant = self.variant.to_dict()

        companions = self.companions

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "job_id": job_id,
                "variant": variant,
                "companions": companions,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.hf_model_variant import HfModelVariant  # noqa: PLC0415

        d = dict(src_dict)
        job_id = d.pop("job_id")

        variant = HfModelVariant.from_dict(d.pop("variant"))

        companions = cast(list[int], d.pop("companions"))

        pull_response = cls(
            job_id=job_id,
            variant=variant,
            companions=companions,
        )

        pull_response.additional_properties = d
        return pull_response

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
