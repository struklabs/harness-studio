from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="MemoryEstimate")


@_attrs_define
class MemoryEstimate:
    """Where the memory goes, in bytes.

    Attributes:
        vram_bytes (int):
        ram_bytes (int):
        model_gpu_bytes (int):
        model_host_bytes (int):
        kv_bytes (int): Whole KV cache (device + host).
        kv_gpu_bytes (int):
        compute_gpu_bytes (int):
        compute_host_bytes (int): Compute buffer in host memory (CPU-only runs).
        mmproj_bytes (int):
        overhead_bytes (int):
        offloaded_layers (int):
        total_layers (int):
        kv_cells (int): KV cells of the full-attention layers (padded context).
        swa_layers (int): Layers with sliding-window attention and their (smaller) cache size.
        swa_cells (int):
        exact (bool): Layer count and KV shape came from the GGUF header (else a size-based guess).
        output_host_bytes (int | Unset): llama.cpp's host-visible logits output buffer (one row per physical batch
            token).
        recurrent_bytes (int | Unset): Recurrent state of all sequences (part of `kv_bytes`; Mamba / gated delta net
            layers).
        checkpoint_bytes (int | Unset): Context checkpoints at most (host memory, part of `ram_bytes`): slots x
            `--ctx-checkpoints` x one window / state copy. 0 for models without SWA / recurrent
            layers (they need none).
        slots (int | Unset): Server slots the estimate assumes.
        draft_gpu_bytes (int | Unset): Draft model (weights + its KV cache + compute) in device memory, part of
            `vram_bytes`.
        draft_host_bytes (int | Unset): Draft model in host memory, part of `ram_bytes`.
    """

    vram_bytes: int
    ram_bytes: int
    model_gpu_bytes: int
    model_host_bytes: int
    kv_bytes: int
    kv_gpu_bytes: int
    compute_gpu_bytes: int
    compute_host_bytes: int
    mmproj_bytes: int
    overhead_bytes: int
    offloaded_layers: int
    total_layers: int
    kv_cells: int
    swa_layers: int
    swa_cells: int
    exact: bool
    output_host_bytes: int | Unset = UNSET
    recurrent_bytes: int | Unset = UNSET
    checkpoint_bytes: int | Unset = UNSET
    slots: int | Unset = UNSET
    draft_gpu_bytes: int | Unset = UNSET
    draft_host_bytes: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        vram_bytes = self.vram_bytes

        ram_bytes = self.ram_bytes

        model_gpu_bytes = self.model_gpu_bytes

        model_host_bytes = self.model_host_bytes

        kv_bytes = self.kv_bytes

        kv_gpu_bytes = self.kv_gpu_bytes

        compute_gpu_bytes = self.compute_gpu_bytes

        compute_host_bytes = self.compute_host_bytes

        mmproj_bytes = self.mmproj_bytes

        overhead_bytes = self.overhead_bytes

        offloaded_layers = self.offloaded_layers

        total_layers = self.total_layers

        kv_cells = self.kv_cells

        swa_layers = self.swa_layers

        swa_cells = self.swa_cells

        exact = self.exact

        output_host_bytes = self.output_host_bytes

        recurrent_bytes = self.recurrent_bytes

        checkpoint_bytes = self.checkpoint_bytes

        slots = self.slots

        draft_gpu_bytes = self.draft_gpu_bytes

        draft_host_bytes = self.draft_host_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "vram_bytes": vram_bytes,
                "ram_bytes": ram_bytes,
                "model_gpu_bytes": model_gpu_bytes,
                "model_host_bytes": model_host_bytes,
                "kv_bytes": kv_bytes,
                "kv_gpu_bytes": kv_gpu_bytes,
                "compute_gpu_bytes": compute_gpu_bytes,
                "compute_host_bytes": compute_host_bytes,
                "mmproj_bytes": mmproj_bytes,
                "overhead_bytes": overhead_bytes,
                "offloaded_layers": offloaded_layers,
                "total_layers": total_layers,
                "kv_cells": kv_cells,
                "swa_layers": swa_layers,
                "swa_cells": swa_cells,
                "exact": exact,
            }
        )
        if output_host_bytes is not UNSET:
            field_dict["output_host_bytes"] = output_host_bytes
        if recurrent_bytes is not UNSET:
            field_dict["recurrent_bytes"] = recurrent_bytes
        if checkpoint_bytes is not UNSET:
            field_dict["checkpoint_bytes"] = checkpoint_bytes
        if slots is not UNSET:
            field_dict["slots"] = slots
        if draft_gpu_bytes is not UNSET:
            field_dict["draft_gpu_bytes"] = draft_gpu_bytes
        if draft_host_bytes is not UNSET:
            field_dict["draft_host_bytes"] = draft_host_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        vram_bytes = d.pop("vram_bytes")

        ram_bytes = d.pop("ram_bytes")

        model_gpu_bytes = d.pop("model_gpu_bytes")

        model_host_bytes = d.pop("model_host_bytes")

        kv_bytes = d.pop("kv_bytes")

        kv_gpu_bytes = d.pop("kv_gpu_bytes")

        compute_gpu_bytes = d.pop("compute_gpu_bytes")

        compute_host_bytes = d.pop("compute_host_bytes")

        mmproj_bytes = d.pop("mmproj_bytes")

        overhead_bytes = d.pop("overhead_bytes")

        offloaded_layers = d.pop("offloaded_layers")

        total_layers = d.pop("total_layers")

        kv_cells = d.pop("kv_cells")

        swa_layers = d.pop("swa_layers")

        swa_cells = d.pop("swa_cells")

        exact = d.pop("exact")

        output_host_bytes = d.pop("output_host_bytes", UNSET)

        recurrent_bytes = d.pop("recurrent_bytes", UNSET)

        checkpoint_bytes = d.pop("checkpoint_bytes", UNSET)

        slots = d.pop("slots", UNSET)

        draft_gpu_bytes = d.pop("draft_gpu_bytes", UNSET)

        draft_host_bytes = d.pop("draft_host_bytes", UNSET)

        memory_estimate = cls(
            vram_bytes=vram_bytes,
            ram_bytes=ram_bytes,
            model_gpu_bytes=model_gpu_bytes,
            model_host_bytes=model_host_bytes,
            kv_bytes=kv_bytes,
            kv_gpu_bytes=kv_gpu_bytes,
            compute_gpu_bytes=compute_gpu_bytes,
            compute_host_bytes=compute_host_bytes,
            mmproj_bytes=mmproj_bytes,
            overhead_bytes=overhead_bytes,
            offloaded_layers=offloaded_layers,
            total_layers=total_layers,
            kv_cells=kv_cells,
            swa_layers=swa_layers,
            swa_cells=swa_cells,
            exact=exact,
            output_host_bytes=output_host_bytes,
            recurrent_bytes=recurrent_bytes,
            checkpoint_bytes=checkpoint_bytes,
            slots=slots,
            draft_gpu_bytes=draft_gpu_bytes,
            draft_host_bytes=draft_host_bytes,
        )

        memory_estimate.additional_properties = d
        return memory_estimate

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
