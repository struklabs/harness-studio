from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.reasoning_settings import ReasoningSettings
    from ..models.sampling_params import SamplingParams


T = TypeVar("T", bound="InferencePreset")


@_attrs_define
class InferencePreset:
    """
    Attributes:
        id (str):
        name (str):
        builtin (bool | Unset): Shipped with the app; read-only (copy it to change it).
        description (str | Unset):
        system_prompt (None | str | Unset):
        sampling (SamplingParams | Unset): Sampling parameters (llama-server `/completion` and `/v1/chat/completions`
            names).
            `None` / empty = not sent (the server's or the model's default applies).
        reasoning (ReasoningSettings | Unset): Reasoning ("thinking") request settings.
        response_format (Any | Unset): OpenAI `response_format` (`{"type":"json_object"}` / `json_schema`).
        tool_choice (Any | Unset): OpenAI `tool_choice` (`"auto"`, `"none"`, `"required"` or a function).
        created_at (int | Unset):
        updated_at (int | Unset):
    """

    id: str
    name: str
    builtin: bool | Unset = UNSET
    description: str | Unset = UNSET
    system_prompt: None | str | Unset = UNSET
    sampling: SamplingParams | Unset = UNSET
    reasoning: ReasoningSettings | Unset = UNSET
    response_format: Any | Unset = UNSET
    tool_choice: Any | Unset = UNSET
    created_at: int | Unset = UNSET
    updated_at: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        builtin = self.builtin

        description = self.description

        system_prompt: None | str | Unset
        if isinstance(self.system_prompt, Unset):
            system_prompt = UNSET
        else:
            system_prompt = self.system_prompt

        sampling: dict[str, Any] | Unset = UNSET
        if not isinstance(self.sampling, Unset):
            sampling = self.sampling.to_dict()

        reasoning: dict[str, Any] | Unset = UNSET
        if not isinstance(self.reasoning, Unset):
            reasoning = self.reasoning.to_dict()

        response_format = self.response_format

        tool_choice = self.tool_choice

        created_at = self.created_at

        updated_at = self.updated_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
            }
        )
        if builtin is not UNSET:
            field_dict["builtin"] = builtin
        if description is not UNSET:
            field_dict["description"] = description
        if system_prompt is not UNSET:
            field_dict["system_prompt"] = system_prompt
        if sampling is not UNSET:
            field_dict["sampling"] = sampling
        if reasoning is not UNSET:
            field_dict["reasoning"] = reasoning
        if response_format is not UNSET:
            field_dict["response_format"] = response_format
        if tool_choice is not UNSET:
            field_dict["tool_choice"] = tool_choice
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.reasoning_settings import ReasoningSettings  # noqa: PLC0415
        from ..models.sampling_params import SamplingParams  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        builtin = d.pop("builtin", UNSET)

        description = d.pop("description", UNSET)

        def _parse_system_prompt(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        system_prompt = _parse_system_prompt(d.pop("system_prompt", UNSET))

        _sampling = d.pop("sampling", UNSET)
        sampling: SamplingParams | Unset
        if isinstance(_sampling, Unset):
            sampling = UNSET
        else:
            sampling = SamplingParams.from_dict(_sampling)

        _reasoning = d.pop("reasoning", UNSET)
        reasoning: ReasoningSettings | Unset
        if isinstance(_reasoning, Unset):
            reasoning = UNSET
        else:
            reasoning = ReasoningSettings.from_dict(_reasoning)

        response_format = d.pop("response_format", UNSET)

        tool_choice = d.pop("tool_choice", UNSET)

        created_at = d.pop("created_at", UNSET)

        updated_at = d.pop("updated_at", UNSET)

        inference_preset = cls(
            id=id,
            name=name,
            builtin=builtin,
            description=description,
            system_prompt=system_prompt,
            sampling=sampling,
            reasoning=reasoning,
            response_format=response_format,
            tool_choice=tool_choice,
            created_at=created_at,
            updated_at=updated_at,
        )

        inference_preset.additional_properties = d
        return inference_preset

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
