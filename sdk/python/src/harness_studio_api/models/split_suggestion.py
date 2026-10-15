from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

T = TypeVar("T", bound="SplitSuggestion")


@_attrs_define
class SplitSuggestion:
    """Main GPU and tensor split proportional to available memory (two or more discrete GPUs).

    Attributes:
        main_gpu (int): Main GPU as an OS adapter index (`LaunchConfig.main_gpu`; see [`pair_adapters`]).
        main_device (str): The main GPU's runtime device (`CUDA0`, `Vulkan1`...).
        tensor_split (str): `--tensor-split` over all runtime devices in order (0 for integrated ones), GiB with
            one decimal; llama.cpp only uses the proportions.
        split_mode (str): `--split-mode`.
        devices (list[str]): Devices that get a share.
    """

    main_gpu: int
    main_device: str
    tensor_split: str
    split_mode: str
    devices: list[str]
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        main_gpu = self.main_gpu

        main_device = self.main_device

        tensor_split = self.tensor_split

        split_mode = self.split_mode

        devices = self.devices

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "main_gpu": main_gpu,
                "main_device": main_device,
                "tensor_split": tensor_split,
                "split_mode": split_mode,
                "devices": devices,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        main_gpu = d.pop("main_gpu")

        main_device = d.pop("main_device")

        tensor_split = d.pop("tensor_split")

        split_mode = d.pop("split_mode")

        devices = cast(list[str], d.pop("devices"))

        split_suggestion = cls(
            main_gpu=main_gpu,
            main_device=main_device,
            tensor_split=tensor_split,
            split_mode=split_mode,
            devices=devices,
        )

        split_suggestion.additional_properties = d
        return split_suggestion

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
