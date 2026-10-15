from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.capability import Capability, check_capability
from ..models.model_format import ModelFormat, check_model_format
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.compatibility_type_0 import CompatibilityType0
    from ..models.compatibility_type_1 import CompatibilityType1
    from ..models.compatibility_type_2 import CompatibilityType2
    from ..models.compatibility_type_3 import CompatibilityType3


T = TypeVar("T", bound="ModelRecord")


@_attrs_define
class ModelRecord:
    """Capability, format and runtime verdict of one model (a local file or a Hub repo).

    Attributes:
        capabilities (list[Capability]):
        format_ (ModelFormat): The format a repo's model files are in.
        runtime (None | str | Unset): Runtime family that runs the format (`None`: no Harness runtime does).
        compatibility (CompatibilityType0 | CompatibilityType1 | CompatibilityType2 | CompatibilityType3 | None |
            Unset):
    """

    capabilities: list[Capability]
    format_: ModelFormat
    runtime: None | str | Unset = UNSET
    compatibility: (
        CompatibilityType0
        | CompatibilityType1
        | CompatibilityType2
        | CompatibilityType3
        | None
        | Unset
    ) = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.compatibility_type_0 import CompatibilityType0  # noqa: PLC0415
        from ..models.compatibility_type_1 import CompatibilityType1  # noqa: PLC0415
        from ..models.compatibility_type_2 import CompatibilityType2  # noqa: PLC0415
        from ..models.compatibility_type_3 import CompatibilityType3  # noqa: PLC0415

        capabilities = []
        for capabilities_item_data in self.capabilities:
            capabilities_item: str = capabilities_item_data
            capabilities.append(capabilities_item)

        format_: str = self.format_

        runtime: None | str | Unset
        if isinstance(self.runtime, Unset):
            runtime = UNSET
        else:
            runtime = self.runtime

        compatibility: dict[str, Any] | None | Unset
        if isinstance(self.compatibility, Unset):
            compatibility = UNSET
        elif isinstance(self.compatibility, CompatibilityType0):
            compatibility = self.compatibility.to_dict()
        elif isinstance(self.compatibility, CompatibilityType1):
            compatibility = self.compatibility.to_dict()
        elif isinstance(self.compatibility, CompatibilityType2):
            compatibility = self.compatibility.to_dict()
        elif isinstance(self.compatibility, CompatibilityType3):
            compatibility = self.compatibility.to_dict()
        else:
            compatibility = self.compatibility

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "capabilities": capabilities,
                "format": format_,
            }
        )
        if runtime is not UNSET:
            field_dict["runtime"] = runtime
        if compatibility is not UNSET:
            field_dict["compatibility"] = compatibility

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.compatibility_type_0 import CompatibilityType0  # noqa: PLC0415
        from ..models.compatibility_type_1 import CompatibilityType1  # noqa: PLC0415
        from ..models.compatibility_type_2 import CompatibilityType2  # noqa: PLC0415
        from ..models.compatibility_type_3 import CompatibilityType3  # noqa: PLC0415

        d = dict(src_dict)
        capabilities = []
        _capabilities = d.pop("capabilities")
        for capabilities_item_data in _capabilities:
            capabilities_item = check_capability(capabilities_item_data)

            capabilities.append(capabilities_item)

        format_ = check_model_format(d.pop("format"))

        def _parse_runtime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        runtime = _parse_runtime(d.pop("runtime", UNSET))

        def _parse_compatibility(
            data: object,
        ) -> (
            CompatibilityType0
            | CompatibilityType1
            | CompatibilityType2
            | CompatibilityType3
            | None
            | Unset
        ):
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_compatibility_type_0 = CompatibilityType0.from_dict(data)

                return componentsschemas_compatibility_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_compatibility_type_1 = CompatibilityType1.from_dict(data)

                return componentsschemas_compatibility_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_compatibility_type_2 = CompatibilityType2.from_dict(data)

                return componentsschemas_compatibility_type_2
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_compatibility_type_3 = CompatibilityType3.from_dict(data)

                return componentsschemas_compatibility_type_3
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                CompatibilityType0
                | CompatibilityType1
                | CompatibilityType2
                | CompatibilityType3
                | None
                | Unset,
                data,
            )

        compatibility = _parse_compatibility(d.pop("compatibility", UNSET))

        model_record = cls(
            capabilities=capabilities,
            format_=format_,
            runtime=runtime,
            compatibility=compatibility,
        )

        model_record.additional_properties = d
        return model_record

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
