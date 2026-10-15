from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.effective_launch_values_modalities import (
        EffectiveLaunchValuesModalities,
    )


T = TypeVar("T", bound="EffectiveLaunchValues")


@_attrs_define
class EffectiveLaunchValues:
    """What the server actually runs with, next to what was requested.

    Attributes:
        requested_ctx (int):
        requested_gpu_layers (int):
        ctx_adjusted (bool): The server picked a different context than requested (`-c 0` or `--fit`).
        observed_at (int): Unix seconds.
        requested_slots (int | None | Unset):
        n_ctx (int | None | Unset): Per-slot context the server uses.
        total_slots (int | None | Unset):
        n_ctx_train (int | None | Unset):
        kv_unified (bool | None | Unset):
        gpu_layers_offloaded (int | None | Unset):
        gpu_layers_total (int | None | Unset):
        gpu_layers_source (None | str | Unset): Where the GPU layer count comes from: `log` (the `offloaded N/M` line,
            printed only
            at `-lv 4` / `--verbose`) or `derived` (llama.cpp's rule applied to `-ngl`, the
            model's `block_count` and the runtime's GPU devices). `None` when unknown.
        gpu_layers_by_fit (bool | Unset): `--fit on` chose the GPU layers (no `-ngl` was passed) and the log did not say
            how many.
        model_alias (None | str | Unset):
        build_info (None | str | Unset):
        chat_format (None | str | Unset):
        reasoning_format (None | str | Unset):
        speculative_types (None | str | Unset):
        modalities (EffectiveLaunchValuesModalities | Unset):
        kv_fa_fallback (None | str | Unset): K/V pair the CUDA build converts to f16 on every token (slow), see
            [`kv_fa_fallback`].
        fit_failed (bool | Unset): llama-server could not fit the settings into free device memory and started anyway
            ([`fit_params_failed`]): on Windows the driver may spill the excess into shared system
            memory.
        shared_memory_slowdown (bool | Unset): Smaller shared-memory pressure, separate from failure to fit.
        shared_memory_bytes (int | None | Unset):
    """

    requested_ctx: int
    requested_gpu_layers: int
    ctx_adjusted: bool
    observed_at: int
    requested_slots: int | None | Unset = UNSET
    n_ctx: int | None | Unset = UNSET
    total_slots: int | None | Unset = UNSET
    n_ctx_train: int | None | Unset = UNSET
    kv_unified: bool | None | Unset = UNSET
    gpu_layers_offloaded: int | None | Unset = UNSET
    gpu_layers_total: int | None | Unset = UNSET
    gpu_layers_source: None | str | Unset = UNSET
    gpu_layers_by_fit: bool | Unset = UNSET
    model_alias: None | str | Unset = UNSET
    build_info: None | str | Unset = UNSET
    chat_format: None | str | Unset = UNSET
    reasoning_format: None | str | Unset = UNSET
    speculative_types: None | str | Unset = UNSET
    modalities: EffectiveLaunchValuesModalities | Unset = UNSET
    kv_fa_fallback: None | str | Unset = UNSET
    fit_failed: bool | Unset = UNSET
    shared_memory_slowdown: bool | Unset = UNSET
    shared_memory_bytes: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        requested_ctx = self.requested_ctx

        requested_gpu_layers = self.requested_gpu_layers

        ctx_adjusted = self.ctx_adjusted

        observed_at = self.observed_at

        requested_slots: int | None | Unset
        if isinstance(self.requested_slots, Unset):
            requested_slots = UNSET
        else:
            requested_slots = self.requested_slots

        n_ctx: int | None | Unset
        if isinstance(self.n_ctx, Unset):
            n_ctx = UNSET
        else:
            n_ctx = self.n_ctx

        total_slots: int | None | Unset
        if isinstance(self.total_slots, Unset):
            total_slots = UNSET
        else:
            total_slots = self.total_slots

        n_ctx_train: int | None | Unset
        if isinstance(self.n_ctx_train, Unset):
            n_ctx_train = UNSET
        else:
            n_ctx_train = self.n_ctx_train

        kv_unified: bool | None | Unset
        if isinstance(self.kv_unified, Unset):
            kv_unified = UNSET
        else:
            kv_unified = self.kv_unified

        gpu_layers_offloaded: int | None | Unset
        if isinstance(self.gpu_layers_offloaded, Unset):
            gpu_layers_offloaded = UNSET
        else:
            gpu_layers_offloaded = self.gpu_layers_offloaded

        gpu_layers_total: int | None | Unset
        if isinstance(self.gpu_layers_total, Unset):
            gpu_layers_total = UNSET
        else:
            gpu_layers_total = self.gpu_layers_total

        gpu_layers_source: None | str | Unset
        if isinstance(self.gpu_layers_source, Unset):
            gpu_layers_source = UNSET
        else:
            gpu_layers_source = self.gpu_layers_source

        gpu_layers_by_fit = self.gpu_layers_by_fit

        model_alias: None | str | Unset
        if isinstance(self.model_alias, Unset):
            model_alias = UNSET
        else:
            model_alias = self.model_alias

        build_info: None | str | Unset
        if isinstance(self.build_info, Unset):
            build_info = UNSET
        else:
            build_info = self.build_info

        chat_format: None | str | Unset
        if isinstance(self.chat_format, Unset):
            chat_format = UNSET
        else:
            chat_format = self.chat_format

        reasoning_format: None | str | Unset
        if isinstance(self.reasoning_format, Unset):
            reasoning_format = UNSET
        else:
            reasoning_format = self.reasoning_format

        speculative_types: None | str | Unset
        if isinstance(self.speculative_types, Unset):
            speculative_types = UNSET
        else:
            speculative_types = self.speculative_types

        modalities: dict[str, Any] | Unset = UNSET
        if not isinstance(self.modalities, Unset):
            modalities = self.modalities.to_dict()

        kv_fa_fallback: None | str | Unset
        if isinstance(self.kv_fa_fallback, Unset):
            kv_fa_fallback = UNSET
        else:
            kv_fa_fallback = self.kv_fa_fallback

        fit_failed = self.fit_failed

        shared_memory_slowdown = self.shared_memory_slowdown

        shared_memory_bytes: int | None | Unset
        if isinstance(self.shared_memory_bytes, Unset):
            shared_memory_bytes = UNSET
        else:
            shared_memory_bytes = self.shared_memory_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "requested_ctx": requested_ctx,
                "requested_gpu_layers": requested_gpu_layers,
                "ctx_adjusted": ctx_adjusted,
                "observed_at": observed_at,
            }
        )
        if requested_slots is not UNSET:
            field_dict["requested_slots"] = requested_slots
        if n_ctx is not UNSET:
            field_dict["n_ctx"] = n_ctx
        if total_slots is not UNSET:
            field_dict["total_slots"] = total_slots
        if n_ctx_train is not UNSET:
            field_dict["n_ctx_train"] = n_ctx_train
        if kv_unified is not UNSET:
            field_dict["kv_unified"] = kv_unified
        if gpu_layers_offloaded is not UNSET:
            field_dict["gpu_layers_offloaded"] = gpu_layers_offloaded
        if gpu_layers_total is not UNSET:
            field_dict["gpu_layers_total"] = gpu_layers_total
        if gpu_layers_source is not UNSET:
            field_dict["gpu_layers_source"] = gpu_layers_source
        if gpu_layers_by_fit is not UNSET:
            field_dict["gpu_layers_by_fit"] = gpu_layers_by_fit
        if model_alias is not UNSET:
            field_dict["model_alias"] = model_alias
        if build_info is not UNSET:
            field_dict["build_info"] = build_info
        if chat_format is not UNSET:
            field_dict["chat_format"] = chat_format
        if reasoning_format is not UNSET:
            field_dict["reasoning_format"] = reasoning_format
        if speculative_types is not UNSET:
            field_dict["speculative_types"] = speculative_types
        if modalities is not UNSET:
            field_dict["modalities"] = modalities
        if kv_fa_fallback is not UNSET:
            field_dict["kv_fa_fallback"] = kv_fa_fallback
        if fit_failed is not UNSET:
            field_dict["fit_failed"] = fit_failed
        if shared_memory_slowdown is not UNSET:
            field_dict["shared_memory_slowdown"] = shared_memory_slowdown
        if shared_memory_bytes is not UNSET:
            field_dict["shared_memory_bytes"] = shared_memory_bytes

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.effective_launch_values_modalities import (
            EffectiveLaunchValuesModalities,  # noqa: PLC0415
        )

        d = dict(src_dict)
        requested_ctx = d.pop("requested_ctx")

        requested_gpu_layers = d.pop("requested_gpu_layers")

        ctx_adjusted = d.pop("ctx_adjusted")

        observed_at = d.pop("observed_at")

        def _parse_requested_slots(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requested_slots = _parse_requested_slots(d.pop("requested_slots", UNSET))

        def _parse_n_ctx(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        n_ctx = _parse_n_ctx(d.pop("n_ctx", UNSET))

        def _parse_total_slots(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        total_slots = _parse_total_slots(d.pop("total_slots", UNSET))

        def _parse_n_ctx_train(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        n_ctx_train = _parse_n_ctx_train(d.pop("n_ctx_train", UNSET))

        def _parse_kv_unified(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        kv_unified = _parse_kv_unified(d.pop("kv_unified", UNSET))

        def _parse_gpu_layers_offloaded(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        gpu_layers_offloaded = _parse_gpu_layers_offloaded(d.pop("gpu_layers_offloaded", UNSET))

        def _parse_gpu_layers_total(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        gpu_layers_total = _parse_gpu_layers_total(d.pop("gpu_layers_total", UNSET))

        def _parse_gpu_layers_source(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        gpu_layers_source = _parse_gpu_layers_source(d.pop("gpu_layers_source", UNSET))

        gpu_layers_by_fit = d.pop("gpu_layers_by_fit", UNSET)

        def _parse_model_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model_alias = _parse_model_alias(d.pop("model_alias", UNSET))

        def _parse_build_info(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        build_info = _parse_build_info(d.pop("build_info", UNSET))

        def _parse_chat_format(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        chat_format = _parse_chat_format(d.pop("chat_format", UNSET))

        def _parse_reasoning_format(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reasoning_format = _parse_reasoning_format(d.pop("reasoning_format", UNSET))

        def _parse_speculative_types(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        speculative_types = _parse_speculative_types(d.pop("speculative_types", UNSET))

        _modalities = d.pop("modalities", UNSET)
        modalities: EffectiveLaunchValuesModalities | Unset
        if isinstance(_modalities, Unset):
            modalities = UNSET
        else:
            modalities = EffectiveLaunchValuesModalities.from_dict(_modalities)

        def _parse_kv_fa_fallback(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        kv_fa_fallback = _parse_kv_fa_fallback(d.pop("kv_fa_fallback", UNSET))

        fit_failed = d.pop("fit_failed", UNSET)

        shared_memory_slowdown = d.pop("shared_memory_slowdown", UNSET)

        def _parse_shared_memory_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        shared_memory_bytes = _parse_shared_memory_bytes(d.pop("shared_memory_bytes", UNSET))

        effective_launch_values = cls(
            requested_ctx=requested_ctx,
            requested_gpu_layers=requested_gpu_layers,
            ctx_adjusted=ctx_adjusted,
            observed_at=observed_at,
            requested_slots=requested_slots,
            n_ctx=n_ctx,
            total_slots=total_slots,
            n_ctx_train=n_ctx_train,
            kv_unified=kv_unified,
            gpu_layers_offloaded=gpu_layers_offloaded,
            gpu_layers_total=gpu_layers_total,
            gpu_layers_source=gpu_layers_source,
            gpu_layers_by_fit=gpu_layers_by_fit,
            model_alias=model_alias,
            build_info=build_info,
            chat_format=chat_format,
            reasoning_format=reasoning_format,
            speculative_types=speculative_types,
            modalities=modalities,
            kv_fa_fallback=kv_fa_fallback,
            fit_failed=fit_failed,
            shared_memory_slowdown=shared_memory_slowdown,
            shared_memory_bytes=shared_memory_bytes,
        )

        effective_launch_values.additional_properties = d
        return effective_launch_values

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
