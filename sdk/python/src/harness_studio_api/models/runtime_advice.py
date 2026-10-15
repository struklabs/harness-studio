from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.fork_hint import ForkHint
    from ..models.runtime_fit import RuntimeFit
    from ..models.tensor_type import TensorType


T = TypeVar("T", bound="RuntimeAdvice")


@_attrs_define
class RuntimeAdvice:
    """Runtime advice for a model (auto-suggestion from the GGUF tensor types).

    Attributes:
        model_path (str):
        special_types (list[TensorType]): Tensor types that not every build loads (empty = nothing to check).
        runtimes (list[RuntimeFit]):
        official_unsupported (bool): The official runtime can't load this file.
        suggested_runtime (None | str | Unset): A registered runtime that loads every tensor type (preferring `current`
            when it does).
        fork (ForkHint | None | Unset):
    """

    model_path: str
    special_types: list[TensorType]
    runtimes: list[RuntimeFit]
    official_unsupported: bool
    suggested_runtime: None | str | Unset = UNSET
    fork: ForkHint | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.fork_hint import ForkHint  # noqa: PLC0415

        model_path = self.model_path

        special_types = []
        for special_types_item_data in self.special_types:
            special_types_item = special_types_item_data.to_dict()
            special_types.append(special_types_item)

        runtimes = []
        for runtimes_item_data in self.runtimes:
            runtimes_item = runtimes_item_data.to_dict()
            runtimes.append(runtimes_item)

        official_unsupported = self.official_unsupported

        suggested_runtime: None | str | Unset
        if isinstance(self.suggested_runtime, Unset):
            suggested_runtime = UNSET
        else:
            suggested_runtime = self.suggested_runtime

        fork: dict[str, Any] | None | Unset
        if isinstance(self.fork, Unset):
            fork = UNSET
        elif isinstance(self.fork, ForkHint):
            fork = self.fork.to_dict()
        else:
            fork = self.fork

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "model_path": model_path,
                "special_types": special_types,
                "runtimes": runtimes,
                "official_unsupported": official_unsupported,
            }
        )
        if suggested_runtime is not UNSET:
            field_dict["suggested_runtime"] = suggested_runtime
        if fork is not UNSET:
            field_dict["fork"] = fork

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.fork_hint import ForkHint  # noqa: PLC0415
        from ..models.runtime_fit import RuntimeFit  # noqa: PLC0415
        from ..models.tensor_type import TensorType  # noqa: PLC0415

        d = dict(src_dict)
        model_path = d.pop("model_path")

        special_types = []
        _special_types = d.pop("special_types")
        for special_types_item_data in _special_types:
            special_types_item = TensorType.from_dict(special_types_item_data)

            special_types.append(special_types_item)

        runtimes = []
        _runtimes = d.pop("runtimes")
        for runtimes_item_data in _runtimes:
            runtimes_item = RuntimeFit.from_dict(runtimes_item_data)

            runtimes.append(runtimes_item)

        official_unsupported = d.pop("official_unsupported")

        def _parse_suggested_runtime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        suggested_runtime = _parse_suggested_runtime(d.pop("suggested_runtime", UNSET))

        def _parse_fork(data: object) -> ForkHint | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                fork_type_1 = ForkHint.from_dict(data)

                return fork_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ForkHint | None | Unset, data)

        fork = _parse_fork(d.pop("fork", UNSET))

        runtime_advice = cls(
            model_path=model_path,
            special_types=special_types,
            runtimes=runtimes,
            official_unsupported=official_unsupported,
            suggested_runtime=suggested_runtime,
            fork=fork,
        )

        runtime_advice.additional_properties = d
        return runtime_advice

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
