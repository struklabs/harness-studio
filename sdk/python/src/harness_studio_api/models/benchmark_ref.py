from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="BenchmarkRef")


@_attrs_define
class BenchmarkRef:
    """A benchmark result next to real usage.

    Attributes:
        run_id (str):
        finished_at (int):
        prompt_tps (float | None | Unset): Best prompt-processing result (`pp` test) of the run, t/s.
        n_prompt (int | None | Unset):
        gen_tps (float | None | Unset): Best generation result (`tg` test) of the run, t/s.
        n_gen (int | None | Unset):
    """

    run_id: str
    finished_at: int
    prompt_tps: float | None | Unset = UNSET
    n_prompt: int | None | Unset = UNSET
    gen_tps: float | None | Unset = UNSET
    n_gen: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        run_id = self.run_id

        finished_at = self.finished_at

        prompt_tps: float | None | Unset
        if isinstance(self.prompt_tps, Unset):
            prompt_tps = UNSET
        else:
            prompt_tps = self.prompt_tps

        n_prompt: int | None | Unset
        if isinstance(self.n_prompt, Unset):
            n_prompt = UNSET
        else:
            n_prompt = self.n_prompt

        gen_tps: float | None | Unset
        if isinstance(self.gen_tps, Unset):
            gen_tps = UNSET
        else:
            gen_tps = self.gen_tps

        n_gen: int | None | Unset
        if isinstance(self.n_gen, Unset):
            n_gen = UNSET
        else:
            n_gen = self.n_gen

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "run_id": run_id,
                "finished_at": finished_at,
            }
        )
        if prompt_tps is not UNSET:
            field_dict["prompt_tps"] = prompt_tps
        if n_prompt is not UNSET:
            field_dict["n_prompt"] = n_prompt
        if gen_tps is not UNSET:
            field_dict["gen_tps"] = gen_tps
        if n_gen is not UNSET:
            field_dict["n_gen"] = n_gen

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        run_id = d.pop("run_id")

        finished_at = d.pop("finished_at")

        def _parse_prompt_tps(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        prompt_tps = _parse_prompt_tps(d.pop("prompt_tps", UNSET))

        def _parse_n_prompt(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        n_prompt = _parse_n_prompt(d.pop("n_prompt", UNSET))

        def _parse_gen_tps(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        gen_tps = _parse_gen_tps(d.pop("gen_tps", UNSET))

        def _parse_n_gen(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        n_gen = _parse_n_gen(d.pop("n_gen", UNSET))

        benchmark_ref = cls(
            run_id=run_id,
            finished_at=finished_at,
            prompt_tps=prompt_tps,
            n_prompt=n_prompt,
            gen_tps=gen_tps,
            n_gen=n_gen,
        )

        benchmark_ref.additional_properties = d
        return benchmark_ref

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
