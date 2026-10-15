from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="SamplingParams")


@_attrs_define
class SamplingParams:
    """Sampling parameters (llama-server `/completion` and `/v1/chat/completions` names).
    `None` / empty = not sent (the server's or the model's default applies).

        Attributes:
            temperature (float | None | Unset):
            top_p (float | None | Unset):
            top_k (int | None | Unset):
            min_p (float | None | Unset):
            typical_p (float | None | Unset):
            top_n_sigma (float | None | Unset):
            repeat_penalty (float | None | Unset):
            repeat_last_n (int | None | Unset):
            presence_penalty (float | None | Unset):
            frequency_penalty (float | None | Unset):
            dry_multiplier (float | None | Unset):
            dry_base (float | None | Unset):
            dry_allowed_length (int | None | Unset):
            dry_penalty_last_n (int | None | Unset):
            dry_sequence_breakers (list[str] | Unset):
            xtc_probability (float | None | Unset):
            xtc_threshold (float | None | Unset):
            mirostat (int | None | Unset): 0 = off, 1 = Mirostat, 2 = Mirostat 2.0.
            mirostat_tau (float | None | Unset):
            mirostat_eta (float | None | Unset):
            dynatemp_range (float | None | Unset): Dynamic temperature: temperature varies by +-range with the token entropy
                (0 = off).
            dynatemp_exponent (float | None | Unset):
            adaptive_target (float | None | Unset): Adaptive-p: target probability of the picked token (negative / unset =
                off).
            adaptive_decay (float | None | Unset):
            logprobs (bool | None | Unset): Return per-token log probabilities (`choices[].logprobs.content`).
            top_logprobs (int | None | Unset): Alternatives per token in `logprobs` (0-20).
            seed (int | None | Unset):
            max_tokens (int | None | Unset): OpenAI name; llama-server also accepts `n_predict`.
            stop (list[str] | Unset):
            samplers (list[str] | Unset): Sampler order (`["top_k", "top_p", "min_p", "temperature"]`...).
            logit_bias (Any | Unset): `{"<token id or text>": bias}` or llama-server's `[[token, bias]]` form.
            grammar (None | str | Unset): GBNF grammar.
            json_schema (Any | Unset): Constrain output to a JSON schema (llama-server `json_schema`).
    """

    temperature: float | None | Unset = UNSET
    top_p: float | None | Unset = UNSET
    top_k: int | None | Unset = UNSET
    min_p: float | None | Unset = UNSET
    typical_p: float | None | Unset = UNSET
    top_n_sigma: float | None | Unset = UNSET
    repeat_penalty: float | None | Unset = UNSET
    repeat_last_n: int | None | Unset = UNSET
    presence_penalty: float | None | Unset = UNSET
    frequency_penalty: float | None | Unset = UNSET
    dry_multiplier: float | None | Unset = UNSET
    dry_base: float | None | Unset = UNSET
    dry_allowed_length: int | None | Unset = UNSET
    dry_penalty_last_n: int | None | Unset = UNSET
    dry_sequence_breakers: list[str] | Unset = UNSET
    xtc_probability: float | None | Unset = UNSET
    xtc_threshold: float | None | Unset = UNSET
    mirostat: int | None | Unset = UNSET
    mirostat_tau: float | None | Unset = UNSET
    mirostat_eta: float | None | Unset = UNSET
    dynatemp_range: float | None | Unset = UNSET
    dynatemp_exponent: float | None | Unset = UNSET
    adaptive_target: float | None | Unset = UNSET
    adaptive_decay: float | None | Unset = UNSET
    logprobs: bool | None | Unset = UNSET
    top_logprobs: int | None | Unset = UNSET
    seed: int | None | Unset = UNSET
    max_tokens: int | None | Unset = UNSET
    stop: list[str] | Unset = UNSET
    samplers: list[str] | Unset = UNSET
    logit_bias: Any | Unset = UNSET
    grammar: None | str | Unset = UNSET
    json_schema: Any | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        temperature: float | None | Unset
        if isinstance(self.temperature, Unset):
            temperature = UNSET
        else:
            temperature = self.temperature

        top_p: float | None | Unset
        if isinstance(self.top_p, Unset):
            top_p = UNSET
        else:
            top_p = self.top_p

        top_k: int | None | Unset
        if isinstance(self.top_k, Unset):
            top_k = UNSET
        else:
            top_k = self.top_k

        min_p: float | None | Unset
        if isinstance(self.min_p, Unset):
            min_p = UNSET
        else:
            min_p = self.min_p

        typical_p: float | None | Unset
        if isinstance(self.typical_p, Unset):
            typical_p = UNSET
        else:
            typical_p = self.typical_p

        top_n_sigma: float | None | Unset
        if isinstance(self.top_n_sigma, Unset):
            top_n_sigma = UNSET
        else:
            top_n_sigma = self.top_n_sigma

        repeat_penalty: float | None | Unset
        if isinstance(self.repeat_penalty, Unset):
            repeat_penalty = UNSET
        else:
            repeat_penalty = self.repeat_penalty

        repeat_last_n: int | None | Unset
        if isinstance(self.repeat_last_n, Unset):
            repeat_last_n = UNSET
        else:
            repeat_last_n = self.repeat_last_n

        presence_penalty: float | None | Unset
        if isinstance(self.presence_penalty, Unset):
            presence_penalty = UNSET
        else:
            presence_penalty = self.presence_penalty

        frequency_penalty: float | None | Unset
        if isinstance(self.frequency_penalty, Unset):
            frequency_penalty = UNSET
        else:
            frequency_penalty = self.frequency_penalty

        dry_multiplier: float | None | Unset
        if isinstance(self.dry_multiplier, Unset):
            dry_multiplier = UNSET
        else:
            dry_multiplier = self.dry_multiplier

        dry_base: float | None | Unset
        if isinstance(self.dry_base, Unset):
            dry_base = UNSET
        else:
            dry_base = self.dry_base

        dry_allowed_length: int | None | Unset
        if isinstance(self.dry_allowed_length, Unset):
            dry_allowed_length = UNSET
        else:
            dry_allowed_length = self.dry_allowed_length

        dry_penalty_last_n: int | None | Unset
        if isinstance(self.dry_penalty_last_n, Unset):
            dry_penalty_last_n = UNSET
        else:
            dry_penalty_last_n = self.dry_penalty_last_n

        dry_sequence_breakers: list[str] | Unset = UNSET
        if not isinstance(self.dry_sequence_breakers, Unset):
            dry_sequence_breakers = self.dry_sequence_breakers

        xtc_probability: float | None | Unset
        if isinstance(self.xtc_probability, Unset):
            xtc_probability = UNSET
        else:
            xtc_probability = self.xtc_probability

        xtc_threshold: float | None | Unset
        if isinstance(self.xtc_threshold, Unset):
            xtc_threshold = UNSET
        else:
            xtc_threshold = self.xtc_threshold

        mirostat: int | None | Unset
        if isinstance(self.mirostat, Unset):
            mirostat = UNSET
        else:
            mirostat = self.mirostat

        mirostat_tau: float | None | Unset
        if isinstance(self.mirostat_tau, Unset):
            mirostat_tau = UNSET
        else:
            mirostat_tau = self.mirostat_tau

        mirostat_eta: float | None | Unset
        if isinstance(self.mirostat_eta, Unset):
            mirostat_eta = UNSET
        else:
            mirostat_eta = self.mirostat_eta

        dynatemp_range: float | None | Unset
        if isinstance(self.dynatemp_range, Unset):
            dynatemp_range = UNSET
        else:
            dynatemp_range = self.dynatemp_range

        dynatemp_exponent: float | None | Unset
        if isinstance(self.dynatemp_exponent, Unset):
            dynatemp_exponent = UNSET
        else:
            dynatemp_exponent = self.dynatemp_exponent

        adaptive_target: float | None | Unset
        if isinstance(self.adaptive_target, Unset):
            adaptive_target = UNSET
        else:
            adaptive_target = self.adaptive_target

        adaptive_decay: float | None | Unset
        if isinstance(self.adaptive_decay, Unset):
            adaptive_decay = UNSET
        else:
            adaptive_decay = self.adaptive_decay

        logprobs: bool | None | Unset
        if isinstance(self.logprobs, Unset):
            logprobs = UNSET
        else:
            logprobs = self.logprobs

        top_logprobs: int | None | Unset
        if isinstance(self.top_logprobs, Unset):
            top_logprobs = UNSET
        else:
            top_logprobs = self.top_logprobs

        seed: int | None | Unset
        if isinstance(self.seed, Unset):
            seed = UNSET
        else:
            seed = self.seed

        max_tokens: int | None | Unset
        if isinstance(self.max_tokens, Unset):
            max_tokens = UNSET
        else:
            max_tokens = self.max_tokens

        stop: list[str] | Unset = UNSET
        if not isinstance(self.stop, Unset):
            stop = self.stop

        samplers: list[str] | Unset = UNSET
        if not isinstance(self.samplers, Unset):
            samplers = self.samplers

        logit_bias = self.logit_bias

        grammar: None | str | Unset
        if isinstance(self.grammar, Unset):
            grammar = UNSET
        else:
            grammar = self.grammar

        json_schema = self.json_schema

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if temperature is not UNSET:
            field_dict["temperature"] = temperature
        if top_p is not UNSET:
            field_dict["top_p"] = top_p
        if top_k is not UNSET:
            field_dict["top_k"] = top_k
        if min_p is not UNSET:
            field_dict["min_p"] = min_p
        if typical_p is not UNSET:
            field_dict["typical_p"] = typical_p
        if top_n_sigma is not UNSET:
            field_dict["top_n_sigma"] = top_n_sigma
        if repeat_penalty is not UNSET:
            field_dict["repeat_penalty"] = repeat_penalty
        if repeat_last_n is not UNSET:
            field_dict["repeat_last_n"] = repeat_last_n
        if presence_penalty is not UNSET:
            field_dict["presence_penalty"] = presence_penalty
        if frequency_penalty is not UNSET:
            field_dict["frequency_penalty"] = frequency_penalty
        if dry_multiplier is not UNSET:
            field_dict["dry_multiplier"] = dry_multiplier
        if dry_base is not UNSET:
            field_dict["dry_base"] = dry_base
        if dry_allowed_length is not UNSET:
            field_dict["dry_allowed_length"] = dry_allowed_length
        if dry_penalty_last_n is not UNSET:
            field_dict["dry_penalty_last_n"] = dry_penalty_last_n
        if dry_sequence_breakers is not UNSET:
            field_dict["dry_sequence_breakers"] = dry_sequence_breakers
        if xtc_probability is not UNSET:
            field_dict["xtc_probability"] = xtc_probability
        if xtc_threshold is not UNSET:
            field_dict["xtc_threshold"] = xtc_threshold
        if mirostat is not UNSET:
            field_dict["mirostat"] = mirostat
        if mirostat_tau is not UNSET:
            field_dict["mirostat_tau"] = mirostat_tau
        if mirostat_eta is not UNSET:
            field_dict["mirostat_eta"] = mirostat_eta
        if dynatemp_range is not UNSET:
            field_dict["dynatemp_range"] = dynatemp_range
        if dynatemp_exponent is not UNSET:
            field_dict["dynatemp_exponent"] = dynatemp_exponent
        if adaptive_target is not UNSET:
            field_dict["adaptive_target"] = adaptive_target
        if adaptive_decay is not UNSET:
            field_dict["adaptive_decay"] = adaptive_decay
        if logprobs is not UNSET:
            field_dict["logprobs"] = logprobs
        if top_logprobs is not UNSET:
            field_dict["top_logprobs"] = top_logprobs
        if seed is not UNSET:
            field_dict["seed"] = seed
        if max_tokens is not UNSET:
            field_dict["max_tokens"] = max_tokens
        if stop is not UNSET:
            field_dict["stop"] = stop
        if samplers is not UNSET:
            field_dict["samplers"] = samplers
        if logit_bias is not UNSET:
            field_dict["logit_bias"] = logit_bias
        if grammar is not UNSET:
            field_dict["grammar"] = grammar
        if json_schema is not UNSET:
            field_dict["json_schema"] = json_schema

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)

        def _parse_temperature(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        temperature = _parse_temperature(d.pop("temperature", UNSET))

        def _parse_top_p(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        top_p = _parse_top_p(d.pop("top_p", UNSET))

        def _parse_top_k(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        top_k = _parse_top_k(d.pop("top_k", UNSET))

        def _parse_min_p(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        min_p = _parse_min_p(d.pop("min_p", UNSET))

        def _parse_typical_p(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        typical_p = _parse_typical_p(d.pop("typical_p", UNSET))

        def _parse_top_n_sigma(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        top_n_sigma = _parse_top_n_sigma(d.pop("top_n_sigma", UNSET))

        def _parse_repeat_penalty(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        repeat_penalty = _parse_repeat_penalty(d.pop("repeat_penalty", UNSET))

        def _parse_repeat_last_n(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        repeat_last_n = _parse_repeat_last_n(d.pop("repeat_last_n", UNSET))

        def _parse_presence_penalty(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        presence_penalty = _parse_presence_penalty(d.pop("presence_penalty", UNSET))

        def _parse_frequency_penalty(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        frequency_penalty = _parse_frequency_penalty(d.pop("frequency_penalty", UNSET))

        def _parse_dry_multiplier(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        dry_multiplier = _parse_dry_multiplier(d.pop("dry_multiplier", UNSET))

        def _parse_dry_base(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        dry_base = _parse_dry_base(d.pop("dry_base", UNSET))

        def _parse_dry_allowed_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        dry_allowed_length = _parse_dry_allowed_length(d.pop("dry_allowed_length", UNSET))

        def _parse_dry_penalty_last_n(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        dry_penalty_last_n = _parse_dry_penalty_last_n(d.pop("dry_penalty_last_n", UNSET))

        dry_sequence_breakers = cast(list[str], d.pop("dry_sequence_breakers", UNSET))

        def _parse_xtc_probability(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        xtc_probability = _parse_xtc_probability(d.pop("xtc_probability", UNSET))

        def _parse_xtc_threshold(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        xtc_threshold = _parse_xtc_threshold(d.pop("xtc_threshold", UNSET))

        def _parse_mirostat(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        mirostat = _parse_mirostat(d.pop("mirostat", UNSET))

        def _parse_mirostat_tau(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        mirostat_tau = _parse_mirostat_tau(d.pop("mirostat_tau", UNSET))

        def _parse_mirostat_eta(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        mirostat_eta = _parse_mirostat_eta(d.pop("mirostat_eta", UNSET))

        def _parse_dynatemp_range(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        dynatemp_range = _parse_dynatemp_range(d.pop("dynatemp_range", UNSET))

        def _parse_dynatemp_exponent(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        dynatemp_exponent = _parse_dynatemp_exponent(d.pop("dynatemp_exponent", UNSET))

        def _parse_adaptive_target(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        adaptive_target = _parse_adaptive_target(d.pop("adaptive_target", UNSET))

        def _parse_adaptive_decay(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        adaptive_decay = _parse_adaptive_decay(d.pop("adaptive_decay", UNSET))

        def _parse_logprobs(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        logprobs = _parse_logprobs(d.pop("logprobs", UNSET))

        def _parse_top_logprobs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        top_logprobs = _parse_top_logprobs(d.pop("top_logprobs", UNSET))

        def _parse_seed(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        seed = _parse_seed(d.pop("seed", UNSET))

        def _parse_max_tokens(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        max_tokens = _parse_max_tokens(d.pop("max_tokens", UNSET))

        stop = cast(list[str], d.pop("stop", UNSET))

        samplers = cast(list[str], d.pop("samplers", UNSET))

        logit_bias = d.pop("logit_bias", UNSET)

        def _parse_grammar(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        grammar = _parse_grammar(d.pop("grammar", UNSET))

        json_schema = d.pop("json_schema", UNSET)

        sampling_params = cls(
            temperature=temperature,
            top_p=top_p,
            top_k=top_k,
            min_p=min_p,
            typical_p=typical_p,
            top_n_sigma=top_n_sigma,
            repeat_penalty=repeat_penalty,
            repeat_last_n=repeat_last_n,
            presence_penalty=presence_penalty,
            frequency_penalty=frequency_penalty,
            dry_multiplier=dry_multiplier,
            dry_base=dry_base,
            dry_allowed_length=dry_allowed_length,
            dry_penalty_last_n=dry_penalty_last_n,
            dry_sequence_breakers=dry_sequence_breakers,
            xtc_probability=xtc_probability,
            xtc_threshold=xtc_threshold,
            mirostat=mirostat,
            mirostat_tau=mirostat_tau,
            mirostat_eta=mirostat_eta,
            dynatemp_range=dynatemp_range,
            dynatemp_exponent=dynatemp_exponent,
            adaptive_target=adaptive_target,
            adaptive_decay=adaptive_decay,
            logprobs=logprobs,
            top_logprobs=top_logprobs,
            seed=seed,
            max_tokens=max_tokens,
            stop=stop,
            samplers=samplers,
            logit_bias=logit_bias,
            grammar=grammar,
            json_schema=json_schema,
        )

        sampling_params.additional_properties = d
        return sampling_params

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
