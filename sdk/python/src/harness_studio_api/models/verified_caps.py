from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.reasoning_control import ReasoningControl


T = TypeVar("T", bound="VerifiedCaps")


@_attrs_define
class VerifiedCaps:
    """Chat-template capabilities as llama-server itself reported them (`/props`
    `chat_template_caps`), recorded by the capability registry on every start.

        Attributes:
            tools (bool): `supports_tools`.
            reasoning (bool): The template takes a reasoning level (`supports_reasoning_effort`) or keeps the
                reasoning of earlier turns (`supports_preserve_reasoning`). Reporting `false` is not
                conclusive (templates that think but drop it from history report it too), so a
                template-detected reasoning ability is not taken back.
            observed_at (int): Unix seconds of that run.
            reasoning_control (ReasoningControl | Unset):
            template_reasoning (bool | None | Unset): The chat template the server really used (`/props.chat_template`) lets
                the model
                reason: the fallback when the flags above stay `false` (hybrid Qwen3 reports neither,
                yet its template switches thinking with `enable_thinking`). `None`: no template sent.
    """

    tools: bool
    reasoning: bool
    observed_at: int
    reasoning_control: ReasoningControl | Unset = UNSET
    template_reasoning: bool | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        tools = self.tools

        reasoning = self.reasoning

        observed_at = self.observed_at

        reasoning_control: dict[str, Any] | Unset = UNSET
        if not isinstance(self.reasoning_control, Unset):
            reasoning_control = self.reasoning_control.to_dict()

        template_reasoning: bool | None | Unset
        if isinstance(self.template_reasoning, Unset):
            template_reasoning = UNSET
        else:
            template_reasoning = self.template_reasoning

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "tools": tools,
                "reasoning": reasoning,
                "observed_at": observed_at,
            }
        )
        if reasoning_control is not UNSET:
            field_dict["reasoning_control"] = reasoning_control
        if template_reasoning is not UNSET:
            field_dict["template_reasoning"] = template_reasoning

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.reasoning_control import ReasoningControl  # noqa: PLC0415

        d = dict(src_dict)
        tools = d.pop("tools")

        reasoning = d.pop("reasoning")

        observed_at = d.pop("observed_at")

        _reasoning_control = d.pop("reasoning_control", UNSET)
        reasoning_control: ReasoningControl | Unset
        if isinstance(_reasoning_control, Unset):
            reasoning_control = UNSET
        else:
            reasoning_control = ReasoningControl.from_dict(_reasoning_control)

        def _parse_template_reasoning(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        template_reasoning = _parse_template_reasoning(d.pop("template_reasoning", UNSET))

        verified_caps = cls(
            tools=tools,
            reasoning=reasoning,
            observed_at=observed_at,
            reasoning_control=reasoning_control,
            template_reasoning=template_reasoning,
        )

        verified_caps.additional_properties = d
        return verified_caps

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
