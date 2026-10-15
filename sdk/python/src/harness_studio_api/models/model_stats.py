from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.benchmark_ref import BenchmarkRef


T = TypeVar("T", bound="ModelStats")


@_attrs_define
class ModelStats:
    """Statistics of one model (alias) over a time range.

    Attributes:
        alias (str): Alias (`*` for the total row; empty for requests that never resolved a model).
        requests (int):
        errors (int):
        ooms (int):
        cancelled (int):
        prompt_tokens (int):
        completion_tokens (int):
        cached_tokens (int):
        percentile_samples (int): Raw rows the latency percentiles are computed from.
        error_rate (float):
        oom_rate (float):
        busy_ms (int): Sum of request durations (time the model was busy for clients).
        loads (int): JIT loads the requests caused.
        definition_id (None | str | Unset):
        avg_gen_tps (float | None | Unset): Generated tokens / generation time over every timed request (token-
            weighted).
        avg_prompt_tps (float | None | Unset): Processed prompt tokens / prompt time (token-weighted).
        avg_ttft_ms (float | None | Unset):
        ttft_p50_ms (float | None | Unset): Percentiles come from raw rows only (the last `retention_days`).
        ttft_p95_ms (float | None | Unset):
        latency_p50_ms (float | None | Unset):
        latency_p95_ms (float | None | Unset):
        vram_avg_bytes (int | None | Unset): Mean of the per-request VRAM peaks.
        vram_peak_bytes (int | None | Unset):
        ram_peak_bytes (int | None | Unset):
        avg_load_ms (float | None | Unset):
        spec_accept_rate (float | None | Unset):
        last_used_ms (int | None | Unset):
        benchmark (BenchmarkRef | None | Unset):
    """

    alias: str
    requests: int
    errors: int
    ooms: int
    cancelled: int
    prompt_tokens: int
    completion_tokens: int
    cached_tokens: int
    percentile_samples: int
    error_rate: float
    oom_rate: float
    busy_ms: int
    loads: int
    definition_id: None | str | Unset = UNSET
    avg_gen_tps: float | None | Unset = UNSET
    avg_prompt_tps: float | None | Unset = UNSET
    avg_ttft_ms: float | None | Unset = UNSET
    ttft_p50_ms: float | None | Unset = UNSET
    ttft_p95_ms: float | None | Unset = UNSET
    latency_p50_ms: float | None | Unset = UNSET
    latency_p95_ms: float | None | Unset = UNSET
    vram_avg_bytes: int | None | Unset = UNSET
    vram_peak_bytes: int | None | Unset = UNSET
    ram_peak_bytes: int | None | Unset = UNSET
    avg_load_ms: float | None | Unset = UNSET
    spec_accept_rate: float | None | Unset = UNSET
    last_used_ms: int | None | Unset = UNSET
    benchmark: BenchmarkRef | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.benchmark_ref import BenchmarkRef  # noqa: PLC0415

        alias = self.alias

        requests = self.requests

        errors = self.errors

        ooms = self.ooms

        cancelled = self.cancelled

        prompt_tokens = self.prompt_tokens

        completion_tokens = self.completion_tokens

        cached_tokens = self.cached_tokens

        percentile_samples = self.percentile_samples

        error_rate = self.error_rate

        oom_rate = self.oom_rate

        busy_ms = self.busy_ms

        loads = self.loads

        definition_id: None | str | Unset
        if isinstance(self.definition_id, Unset):
            definition_id = UNSET
        else:
            definition_id = self.definition_id

        avg_gen_tps: float | None | Unset
        if isinstance(self.avg_gen_tps, Unset):
            avg_gen_tps = UNSET
        else:
            avg_gen_tps = self.avg_gen_tps

        avg_prompt_tps: float | None | Unset
        if isinstance(self.avg_prompt_tps, Unset):
            avg_prompt_tps = UNSET
        else:
            avg_prompt_tps = self.avg_prompt_tps

        avg_ttft_ms: float | None | Unset
        if isinstance(self.avg_ttft_ms, Unset):
            avg_ttft_ms = UNSET
        else:
            avg_ttft_ms = self.avg_ttft_ms

        ttft_p50_ms: float | None | Unset
        if isinstance(self.ttft_p50_ms, Unset):
            ttft_p50_ms = UNSET
        else:
            ttft_p50_ms = self.ttft_p50_ms

        ttft_p95_ms: float | None | Unset
        if isinstance(self.ttft_p95_ms, Unset):
            ttft_p95_ms = UNSET
        else:
            ttft_p95_ms = self.ttft_p95_ms

        latency_p50_ms: float | None | Unset
        if isinstance(self.latency_p50_ms, Unset):
            latency_p50_ms = UNSET
        else:
            latency_p50_ms = self.latency_p50_ms

        latency_p95_ms: float | None | Unset
        if isinstance(self.latency_p95_ms, Unset):
            latency_p95_ms = UNSET
        else:
            latency_p95_ms = self.latency_p95_ms

        vram_avg_bytes: int | None | Unset
        if isinstance(self.vram_avg_bytes, Unset):
            vram_avg_bytes = UNSET
        else:
            vram_avg_bytes = self.vram_avg_bytes

        vram_peak_bytes: int | None | Unset
        if isinstance(self.vram_peak_bytes, Unset):
            vram_peak_bytes = UNSET
        else:
            vram_peak_bytes = self.vram_peak_bytes

        ram_peak_bytes: int | None | Unset
        if isinstance(self.ram_peak_bytes, Unset):
            ram_peak_bytes = UNSET
        else:
            ram_peak_bytes = self.ram_peak_bytes

        avg_load_ms: float | None | Unset
        if isinstance(self.avg_load_ms, Unset):
            avg_load_ms = UNSET
        else:
            avg_load_ms = self.avg_load_ms

        spec_accept_rate: float | None | Unset
        if isinstance(self.spec_accept_rate, Unset):
            spec_accept_rate = UNSET
        else:
            spec_accept_rate = self.spec_accept_rate

        last_used_ms: int | None | Unset
        if isinstance(self.last_used_ms, Unset):
            last_used_ms = UNSET
        else:
            last_used_ms = self.last_used_ms

        benchmark: dict[str, Any] | None | Unset
        if isinstance(self.benchmark, Unset):
            benchmark = UNSET
        elif isinstance(self.benchmark, BenchmarkRef):
            benchmark = self.benchmark.to_dict()
        else:
            benchmark = self.benchmark

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "alias": alias,
                "requests": requests,
                "errors": errors,
                "ooms": ooms,
                "cancelled": cancelled,
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "cached_tokens": cached_tokens,
                "percentile_samples": percentile_samples,
                "error_rate": error_rate,
                "oom_rate": oom_rate,
                "busy_ms": busy_ms,
                "loads": loads,
            }
        )
        if definition_id is not UNSET:
            field_dict["definition_id"] = definition_id
        if avg_gen_tps is not UNSET:
            field_dict["avg_gen_tps"] = avg_gen_tps
        if avg_prompt_tps is not UNSET:
            field_dict["avg_prompt_tps"] = avg_prompt_tps
        if avg_ttft_ms is not UNSET:
            field_dict["avg_ttft_ms"] = avg_ttft_ms
        if ttft_p50_ms is not UNSET:
            field_dict["ttft_p50_ms"] = ttft_p50_ms
        if ttft_p95_ms is not UNSET:
            field_dict["ttft_p95_ms"] = ttft_p95_ms
        if latency_p50_ms is not UNSET:
            field_dict["latency_p50_ms"] = latency_p50_ms
        if latency_p95_ms is not UNSET:
            field_dict["latency_p95_ms"] = latency_p95_ms
        if vram_avg_bytes is not UNSET:
            field_dict["vram_avg_bytes"] = vram_avg_bytes
        if vram_peak_bytes is not UNSET:
            field_dict["vram_peak_bytes"] = vram_peak_bytes
        if ram_peak_bytes is not UNSET:
            field_dict["ram_peak_bytes"] = ram_peak_bytes
        if avg_load_ms is not UNSET:
            field_dict["avg_load_ms"] = avg_load_ms
        if spec_accept_rate is not UNSET:
            field_dict["spec_accept_rate"] = spec_accept_rate
        if last_used_ms is not UNSET:
            field_dict["last_used_ms"] = last_used_ms
        if benchmark is not UNSET:
            field_dict["benchmark"] = benchmark

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.benchmark_ref import BenchmarkRef  # noqa: PLC0415

        d = dict(src_dict)
        alias = d.pop("alias")

        requests = d.pop("requests")

        errors = d.pop("errors")

        ooms = d.pop("ooms")

        cancelled = d.pop("cancelled")

        prompt_tokens = d.pop("prompt_tokens")

        completion_tokens = d.pop("completion_tokens")

        cached_tokens = d.pop("cached_tokens")

        percentile_samples = d.pop("percentile_samples")

        error_rate = d.pop("error_rate")

        oom_rate = d.pop("oom_rate")

        busy_ms = d.pop("busy_ms")

        loads = d.pop("loads")

        def _parse_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        definition_id = _parse_definition_id(d.pop("definition_id", UNSET))

        def _parse_avg_gen_tps(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        avg_gen_tps = _parse_avg_gen_tps(d.pop("avg_gen_tps", UNSET))

        def _parse_avg_prompt_tps(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        avg_prompt_tps = _parse_avg_prompt_tps(d.pop("avg_prompt_tps", UNSET))

        def _parse_avg_ttft_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        avg_ttft_ms = _parse_avg_ttft_ms(d.pop("avg_ttft_ms", UNSET))

        def _parse_ttft_p50_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        ttft_p50_ms = _parse_ttft_p50_ms(d.pop("ttft_p50_ms", UNSET))

        def _parse_ttft_p95_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        ttft_p95_ms = _parse_ttft_p95_ms(d.pop("ttft_p95_ms", UNSET))

        def _parse_latency_p50_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        latency_p50_ms = _parse_latency_p50_ms(d.pop("latency_p50_ms", UNSET))

        def _parse_latency_p95_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        latency_p95_ms = _parse_latency_p95_ms(d.pop("latency_p95_ms", UNSET))

        def _parse_vram_avg_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vram_avg_bytes = _parse_vram_avg_bytes(d.pop("vram_avg_bytes", UNSET))

        def _parse_vram_peak_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        vram_peak_bytes = _parse_vram_peak_bytes(d.pop("vram_peak_bytes", UNSET))

        def _parse_ram_peak_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ram_peak_bytes = _parse_ram_peak_bytes(d.pop("ram_peak_bytes", UNSET))

        def _parse_avg_load_ms(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        avg_load_ms = _parse_avg_load_ms(d.pop("avg_load_ms", UNSET))

        def _parse_spec_accept_rate(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        spec_accept_rate = _parse_spec_accept_rate(d.pop("spec_accept_rate", UNSET))

        def _parse_last_used_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        last_used_ms = _parse_last_used_ms(d.pop("last_used_ms", UNSET))

        def _parse_benchmark(data: object) -> BenchmarkRef | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                benchmark_type_1 = BenchmarkRef.from_dict(data)

                return benchmark_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(BenchmarkRef | None | Unset, data)

        benchmark = _parse_benchmark(d.pop("benchmark", UNSET))

        model_stats = cls(
            alias=alias,
            requests=requests,
            errors=errors,
            ooms=ooms,
            cancelled=cancelled,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            cached_tokens=cached_tokens,
            percentile_samples=percentile_samples,
            error_rate=error_rate,
            oom_rate=oom_rate,
            busy_ms=busy_ms,
            loads=loads,
            definition_id=definition_id,
            avg_gen_tps=avg_gen_tps,
            avg_prompt_tps=avg_prompt_tps,
            avg_ttft_ms=avg_ttft_ms,
            ttft_p50_ms=ttft_p50_ms,
            ttft_p95_ms=ttft_p95_ms,
            latency_p50_ms=latency_p50_ms,
            latency_p95_ms=latency_p95_ms,
            vram_avg_bytes=vram_avg_bytes,
            vram_peak_bytes=vram_peak_bytes,
            ram_peak_bytes=ram_peak_bytes,
            avg_load_ms=avg_load_ms,
            spec_accept_rate=spec_accept_rate,
            last_used_ms=last_used_ms,
            benchmark=benchmark,
        )

        model_stats.additional_properties = d
        return model_stats

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
