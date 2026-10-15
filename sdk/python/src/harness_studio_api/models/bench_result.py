from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="BenchResult")


@_attrs_define
class BenchResult:
    """One llama-bench result row (a subset of its JSON output).

    Attributes:
        model_type (str | Unset):  Default: ''.
        model_size (int | Unset):  Default: 0.
        model_n_params (int | Unset):  Default: 0.
        backends (str | Unset):  Default: ''.
        gpu_info (str | Unset):  Default: ''.
        cpu_info (str | Unset):  Default: ''.
        build_commit (str | Unset):  Default: ''.
        build_number (int | None | Unset):
        n_prompt (int | Unset):  Default: 0.
        n_gen (int | Unset):  Default: 0.
        n_depth (int | Unset):  Default: 0.
        n_gpu_layers (int | Unset):  Default: 0.
        n_threads (int | Unset):  Default: 0.
        n_batch (int | Unset):  Default: 0.
        n_ubatch (int | Unset):  Default: 0.
        type_k (str | Unset):  Default: ''.
        type_v (str | Unset):  Default: ''.
        flash_attn (Any | Unset): -1 auto / 0 off / 1 on (older builds report a bool).
        avg_ts (float | Unset):  Default: 0.0.
        stddev_ts (float | Unset):  Default: 0.0.
    """

    model_type: str | Unset = ""
    model_size: int | Unset = 0
    model_n_params: int | Unset = 0
    backends: str | Unset = ""
    gpu_info: str | Unset = ""
    cpu_info: str | Unset = ""
    build_commit: str | Unset = ""
    build_number: int | None | Unset = UNSET
    n_prompt: int | Unset = 0
    n_gen: int | Unset = 0
    n_depth: int | Unset = 0
    n_gpu_layers: int | Unset = 0
    n_threads: int | Unset = 0
    n_batch: int | Unset = 0
    n_ubatch: int | Unset = 0
    type_k: str | Unset = ""
    type_v: str | Unset = ""
    flash_attn: Any | Unset = UNSET
    avg_ts: float | Unset = 0.0
    stddev_ts: float | Unset = 0.0
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model_type = self.model_type

        model_size = self.model_size

        model_n_params = self.model_n_params

        backends = self.backends

        gpu_info = self.gpu_info

        cpu_info = self.cpu_info

        build_commit = self.build_commit

        build_number: int | None | Unset
        if isinstance(self.build_number, Unset):
            build_number = UNSET
        else:
            build_number = self.build_number

        n_prompt = self.n_prompt

        n_gen = self.n_gen

        n_depth = self.n_depth

        n_gpu_layers = self.n_gpu_layers

        n_threads = self.n_threads

        n_batch = self.n_batch

        n_ubatch = self.n_ubatch

        type_k = self.type_k

        type_v = self.type_v

        flash_attn = self.flash_attn

        avg_ts = self.avg_ts

        stddev_ts = self.stddev_ts

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if model_type is not UNSET:
            field_dict["model_type"] = model_type
        if model_size is not UNSET:
            field_dict["model_size"] = model_size
        if model_n_params is not UNSET:
            field_dict["model_n_params"] = model_n_params
        if backends is not UNSET:
            field_dict["backends"] = backends
        if gpu_info is not UNSET:
            field_dict["gpu_info"] = gpu_info
        if cpu_info is not UNSET:
            field_dict["cpu_info"] = cpu_info
        if build_commit is not UNSET:
            field_dict["build_commit"] = build_commit
        if build_number is not UNSET:
            field_dict["build_number"] = build_number
        if n_prompt is not UNSET:
            field_dict["n_prompt"] = n_prompt
        if n_gen is not UNSET:
            field_dict["n_gen"] = n_gen
        if n_depth is not UNSET:
            field_dict["n_depth"] = n_depth
        if n_gpu_layers is not UNSET:
            field_dict["n_gpu_layers"] = n_gpu_layers
        if n_threads is not UNSET:
            field_dict["n_threads"] = n_threads
        if n_batch is not UNSET:
            field_dict["n_batch"] = n_batch
        if n_ubatch is not UNSET:
            field_dict["n_ubatch"] = n_ubatch
        if type_k is not UNSET:
            field_dict["type_k"] = type_k
        if type_v is not UNSET:
            field_dict["type_v"] = type_v
        if flash_attn is not UNSET:
            field_dict["flash_attn"] = flash_attn
        if avg_ts is not UNSET:
            field_dict["avg_ts"] = avg_ts
        if stddev_ts is not UNSET:
            field_dict["stddev_ts"] = stddev_ts

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        model_type = d.pop("model_type", UNSET)

        model_size = d.pop("model_size", UNSET)

        model_n_params = d.pop("model_n_params", UNSET)

        backends = d.pop("backends", UNSET)

        gpu_info = d.pop("gpu_info", UNSET)

        cpu_info = d.pop("cpu_info", UNSET)

        build_commit = d.pop("build_commit", UNSET)

        def _parse_build_number(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        build_number = _parse_build_number(d.pop("build_number", UNSET))

        n_prompt = d.pop("n_prompt", UNSET)

        n_gen = d.pop("n_gen", UNSET)

        n_depth = d.pop("n_depth", UNSET)

        n_gpu_layers = d.pop("n_gpu_layers", UNSET)

        n_threads = d.pop("n_threads", UNSET)

        n_batch = d.pop("n_batch", UNSET)

        n_ubatch = d.pop("n_ubatch", UNSET)

        type_k = d.pop("type_k", UNSET)

        type_v = d.pop("type_v", UNSET)

        flash_attn = d.pop("flash_attn", UNSET)

        avg_ts = d.pop("avg_ts", UNSET)

        stddev_ts = d.pop("stddev_ts", UNSET)

        bench_result = cls(
            model_type=model_type,
            model_size=model_size,
            model_n_params=model_n_params,
            backends=backends,
            gpu_info=gpu_info,
            cpu_info=cpu_info,
            build_commit=build_commit,
            build_number=build_number,
            n_prompt=n_prompt,
            n_gen=n_gen,
            n_depth=n_depth,
            n_gpu_layers=n_gpu_layers,
            n_threads=n_threads,
            n_batch=n_batch,
            n_ubatch=n_ubatch,
            type_k=type_k,
            type_v=type_v,
            flash_attn=flash_attn,
            avg_ts=avg_ts,
            stddev_ts=stddev_ts,
        )

        bench_result.additional_properties = d
        return bench_result

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
