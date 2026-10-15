from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="BenchmarkRequest")


@_attrs_define
class BenchmarkRequest:
    """
    Attributes:
        model_path (str | Unset):  Default: ''.
        n_prompt (list[int] | Unset): Each list is a sweep: llama-bench runs every combination.
        n_gen (list[int] | Unset):
        n_depth (list[int] | Unset):
        gpu_layers (list[int] | Unset):
        flash_attn (list[str] | Unset): "on" | "off" | "auto"
        cache_type_k (list[str] | Unset):
        cache_type_v (list[str] | Unset):
        threads (int | None | Unset):
        batch_size (int | None | Unset):
        ubatch_size (int | None | Unset):
        repetitions (int | None | Unset):
        concurrent_servers (int | Unset): Servers that held GPU memory when the run started (set by the front end that
            knows the
            registry, not by the caller): their load skews the numbers, so the saved run keeps it. Default: 0.
    """

    model_path: str | Unset = ""
    n_prompt: list[int] | Unset = UNSET
    n_gen: list[int] | Unset = UNSET
    n_depth: list[int] | Unset = UNSET
    gpu_layers: list[int] | Unset = UNSET
    flash_attn: list[str] | Unset = UNSET
    cache_type_k: list[str] | Unset = UNSET
    cache_type_v: list[str] | Unset = UNSET
    threads: int | None | Unset = UNSET
    batch_size: int | None | Unset = UNSET
    ubatch_size: int | None | Unset = UNSET
    repetitions: int | None | Unset = UNSET
    concurrent_servers: int | Unset = 0
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model_path = self.model_path

        n_prompt: list[int] | Unset = UNSET
        if not isinstance(self.n_prompt, Unset):
            n_prompt = self.n_prompt

        n_gen: list[int] | Unset = UNSET
        if not isinstance(self.n_gen, Unset):
            n_gen = self.n_gen

        n_depth: list[int] | Unset = UNSET
        if not isinstance(self.n_depth, Unset):
            n_depth = self.n_depth

        gpu_layers: list[int] | Unset = UNSET
        if not isinstance(self.gpu_layers, Unset):
            gpu_layers = self.gpu_layers

        flash_attn: list[str] | Unset = UNSET
        if not isinstance(self.flash_attn, Unset):
            flash_attn = self.flash_attn

        cache_type_k: list[str] | Unset = UNSET
        if not isinstance(self.cache_type_k, Unset):
            cache_type_k = self.cache_type_k

        cache_type_v: list[str] | Unset = UNSET
        if not isinstance(self.cache_type_v, Unset):
            cache_type_v = self.cache_type_v

        threads: int | None | Unset
        if isinstance(self.threads, Unset):
            threads = UNSET
        else:
            threads = self.threads

        batch_size: int | None | Unset
        if isinstance(self.batch_size, Unset):
            batch_size = UNSET
        else:
            batch_size = self.batch_size

        ubatch_size: int | None | Unset
        if isinstance(self.ubatch_size, Unset):
            ubatch_size = UNSET
        else:
            ubatch_size = self.ubatch_size

        repetitions: int | None | Unset
        if isinstance(self.repetitions, Unset):
            repetitions = UNSET
        else:
            repetitions = self.repetitions

        concurrent_servers = self.concurrent_servers

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if model_path is not UNSET:
            field_dict["model_path"] = model_path
        if n_prompt is not UNSET:
            field_dict["n_prompt"] = n_prompt
        if n_gen is not UNSET:
            field_dict["n_gen"] = n_gen
        if n_depth is not UNSET:
            field_dict["n_depth"] = n_depth
        if gpu_layers is not UNSET:
            field_dict["gpu_layers"] = gpu_layers
        if flash_attn is not UNSET:
            field_dict["flash_attn"] = flash_attn
        if cache_type_k is not UNSET:
            field_dict["cache_type_k"] = cache_type_k
        if cache_type_v is not UNSET:
            field_dict["cache_type_v"] = cache_type_v
        if threads is not UNSET:
            field_dict["threads"] = threads
        if batch_size is not UNSET:
            field_dict["batch_size"] = batch_size
        if ubatch_size is not UNSET:
            field_dict["ubatch_size"] = ubatch_size
        if repetitions is not UNSET:
            field_dict["repetitions"] = repetitions
        if concurrent_servers is not UNSET:
            field_dict["concurrent_servers"] = concurrent_servers

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        model_path = d.pop("model_path", UNSET)

        n_prompt = cast(list[int], d.pop("n_prompt", UNSET))

        n_gen = cast(list[int], d.pop("n_gen", UNSET))

        n_depth = cast(list[int], d.pop("n_depth", UNSET))

        gpu_layers = cast(list[int], d.pop("gpu_layers", UNSET))

        flash_attn = cast(list[str], d.pop("flash_attn", UNSET))

        cache_type_k = cast(list[str], d.pop("cache_type_k", UNSET))

        cache_type_v = cast(list[str], d.pop("cache_type_v", UNSET))

        def _parse_threads(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        threads = _parse_threads(d.pop("threads", UNSET))

        def _parse_batch_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        batch_size = _parse_batch_size(d.pop("batch_size", UNSET))

        def _parse_ubatch_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ubatch_size = _parse_ubatch_size(d.pop("ubatch_size", UNSET))

        def _parse_repetitions(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        repetitions = _parse_repetitions(d.pop("repetitions", UNSET))

        concurrent_servers = d.pop("concurrent_servers", UNSET)

        benchmark_request = cls(
            model_path=model_path,
            n_prompt=n_prompt,
            n_gen=n_gen,
            n_depth=n_depth,
            gpu_layers=gpu_layers,
            flash_attn=flash_attn,
            cache_type_k=cache_type_k,
            cache_type_v=cache_type_v,
            threads=threads,
            batch_size=batch_size,
            ubatch_size=ubatch_size,
            repetitions=repetitions,
            concurrent_servers=concurrent_servers,
        )

        benchmark_request.additional_properties = d
        return benchmark_request

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
