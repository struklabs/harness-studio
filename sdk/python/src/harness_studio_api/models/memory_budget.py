from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="MemoryBudget")


@_attrs_define
class MemoryBudget:
    """Memory usable for the model.

    Attributes:
        vram_bytes (int): Device memory for offload (0 = CPU only).
        ram_bytes (int):
        unified (bool): Apple Silicon: `vram_bytes` is the GPU share of unified memory.
        shared_igpu (bool | Unset): Integrated GPU without dedicated memory (Intel HD/UHD/Iris, AMD APU): `vram_bytes`
            is
            the part of system RAM the driver lets it use, the same pool as `ram_bytes`.
        igpu_offload_faster (bool | None | Unset): Integrated GPU: whether offloading the weights generates faster than
            keeping them on
            the CPU. From a llama-bench run of this model with both `-ngl 0` and a full offload
            (Performance > Benchmark); otherwise from the device class (older Intel HD/UHD/Iris:
            no). `None`: unknown, the memory budget decides.
        cuda (bool | Unset): The machine's backend is CUDA: its default build has no flash-attention kernel for
            mixed K/V cache types (see [`crate::kv_policy`]).
        rocm (bool | Unset): ROCm/HIP: matching K/V pairs avoid the measured slow generation path.
    """

    vram_bytes: int
    ram_bytes: int
    unified: bool
    shared_igpu: bool | Unset = UNSET
    igpu_offload_faster: bool | None | Unset = UNSET
    cuda: bool | Unset = UNSET
    rocm: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        vram_bytes = self.vram_bytes

        ram_bytes = self.ram_bytes

        unified = self.unified

        shared_igpu = self.shared_igpu

        igpu_offload_faster: bool | None | Unset
        if isinstance(self.igpu_offload_faster, Unset):
            igpu_offload_faster = UNSET
        else:
            igpu_offload_faster = self.igpu_offload_faster

        cuda = self.cuda

        rocm = self.rocm

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "vram_bytes": vram_bytes,
                "ram_bytes": ram_bytes,
                "unified": unified,
            }
        )
        if shared_igpu is not UNSET:
            field_dict["shared_igpu"] = shared_igpu
        if igpu_offload_faster is not UNSET:
            field_dict["igpu_offload_faster"] = igpu_offload_faster
        if cuda is not UNSET:
            field_dict["cuda"] = cuda
        if rocm is not UNSET:
            field_dict["rocm"] = rocm

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        vram_bytes = d.pop("vram_bytes")

        ram_bytes = d.pop("ram_bytes")

        unified = d.pop("unified")

        shared_igpu = d.pop("shared_igpu", UNSET)

        def _parse_igpu_offload_faster(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        igpu_offload_faster = _parse_igpu_offload_faster(d.pop("igpu_offload_faster", UNSET))

        cuda = d.pop("cuda", UNSET)

        rocm = d.pop("rocm", UNSET)

        memory_budget = cls(
            vram_bytes=vram_bytes,
            ram_bytes=ram_bytes,
            unified=unified,
            shared_igpu=shared_igpu,
            igpu_offload_faster=igpu_offload_faster,
            cuda=cuda,
            rocm=rocm,
        )

        memory_budget.additional_properties = d
        return memory_budget

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
