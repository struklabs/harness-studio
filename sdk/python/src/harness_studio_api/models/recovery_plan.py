from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.recovery_option import RecoveryOption
    from ..models.server_failure import ServerFailure


T = TypeVar("T", bound="RecoveryPlan")


@_attrs_define
class RecoveryPlan:
    """The options for a failed server plus what they are based on.

    Attributes:
        server_id (str):
        failure (ServerFailure): A classified failure of one run.
        options (list[RecoveryOption]): Best first.
        free_vram_bytes (int | None | Unset): Free memory of the target GPU when the plan was made.
        estimated_need_bytes (int | None | Unset): Estimated need of the failed launch on the GPU (weights + KV), bytes.
    """

    server_id: str
    failure: ServerFailure
    options: list[RecoveryOption]
    free_vram_bytes: int | None | Unset = UNSET
    estimated_need_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        server_id = self.server_id

        failure = self.failure.to_dict()

        options = []
        for options_item_data in self.options:
            options_item = options_item_data.to_dict()
            options.append(options_item)

        free_vram_bytes: int | None | Unset
        if isinstance(self.free_vram_bytes, Unset):
            free_vram_bytes = UNSET
        else:
            free_vram_bytes = self.free_vram_bytes

        estimated_need_bytes: int | None | Unset
        if isinstance(self.estimated_need_bytes, Unset):
            estimated_need_bytes = UNSET
        else:
            estimated_need_bytes = self.estimated_need_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "server_id": server_id,
                "failure": failure,
                "options": options,
            }
        )
        if free_vram_bytes is not UNSET:
            field_dict["free_vram_bytes"] = free_vram_bytes
        if estimated_need_bytes is not UNSET:
            field_dict["estimated_need_bytes"] = estimated_need_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.recovery_option import RecoveryOption  # noqa: PLC0415
        from ..models.server_failure import ServerFailure  # noqa: PLC0415

        d = dict(src_dict)
        server_id = d.pop("server_id")

        failure = ServerFailure.from_dict(d.pop("failure"))

        options = []
        _options = d.pop("options")
        for options_item_data in _options:
            options_item = RecoveryOption.from_dict(options_item_data)

            options.append(options_item)

        def _parse_free_vram_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        free_vram_bytes = _parse_free_vram_bytes(d.pop("free_vram_bytes", UNSET))

        def _parse_estimated_need_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        estimated_need_bytes = _parse_estimated_need_bytes(d.pop("estimated_need_bytes", UNSET))

        recovery_plan = cls(
            server_id=server_id,
            failure=failure,
            options=options,
            free_vram_bytes=free_vram_bytes,
            estimated_need_bytes=estimated_need_bytes,
        )

        recovery_plan.additional_properties = d
        return recovery_plan

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
