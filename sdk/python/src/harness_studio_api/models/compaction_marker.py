from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.compaction_strategy import CompactionStrategy, check_compaction_strategy
from ..models.compaction_trigger import CompactionTrigger, check_compaction_trigger
from ..types import UNSET, Unset

T = TypeVar("T", bound="CompactionMarker")


@_attrs_define
class CompactionMarker:
    """The compaction marker stored in `meta.compaction` of the marker message's variant.

    Attributes:
        version (int):
        boundary_id (str): Last message the summary covers; the request continues after it.
        covered (int): Conversation messages the summary covers (including what an earlier summary covered).
        turns (int): Turns summarized by this compaction.
        kept_turns (int): Turns kept verbatim after the boundary when it was made.
        trigger (CompactionTrigger): What started a compaction.
        strategy (CompactionStrategy): How the summary is produced.
        tokens_before (int): Prompt tokens of the conversation before / after (system, summary, kept turns).
        tokens_after (int):
        summary_tokens (int):
        created_at (int):
        instruction (None | str | Unset):
        model (None | str | Unset): Model that wrote the summary.
        tool_results_cleared (int | Unset): Tool results shortened before summarizing.
        exact_tokens (bool | Unset): Token counts came from the server's tokenizer (else estimated).
        previous_id (None | str | Unset): Earlier marker whose summary was folded into this one.
        undone (bool | Unset): Undone by the user: the request ignores it (and auto-compaction pauses on this path).
        mid_turn (bool | Unset): Made while an answer was still running (between two tool rounds), see
            [`plan_mid_turn`].
        pinned_id (None | str | Unset): The user message of the running turn: it lies before the boundary but is still
            sent
            verbatim right after the summary (a request must keep the question it answers).
    """

    version: int
    boundary_id: str
    covered: int
    turns: int
    kept_turns: int
    trigger: CompactionTrigger
    strategy: CompactionStrategy
    tokens_before: int
    tokens_after: int
    summary_tokens: int
    created_at: int
    instruction: None | str | Unset = UNSET
    model: None | str | Unset = UNSET
    tool_results_cleared: int | Unset = UNSET
    exact_tokens: bool | Unset = UNSET
    previous_id: None | str | Unset = UNSET
    undone: bool | Unset = UNSET
    mid_turn: bool | Unset = UNSET
    pinned_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        version = self.version

        boundary_id = self.boundary_id

        covered = self.covered

        turns = self.turns

        kept_turns = self.kept_turns

        trigger: str = self.trigger

        strategy: str = self.strategy

        tokens_before = self.tokens_before

        tokens_after = self.tokens_after

        summary_tokens = self.summary_tokens

        created_at = self.created_at

        instruction: None | str | Unset
        if isinstance(self.instruction, Unset):
            instruction = UNSET
        else:
            instruction = self.instruction

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        tool_results_cleared = self.tool_results_cleared

        exact_tokens = self.exact_tokens

        previous_id: None | str | Unset
        if isinstance(self.previous_id, Unset):
            previous_id = UNSET
        else:
            previous_id = self.previous_id

        undone = self.undone

        mid_turn = self.mid_turn

        pinned_id: None | str | Unset
        if isinstance(self.pinned_id, Unset):
            pinned_id = UNSET
        else:
            pinned_id = self.pinned_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "version": version,
                "boundary_id": boundary_id,
                "covered": covered,
                "turns": turns,
                "kept_turns": kept_turns,
                "trigger": trigger,
                "strategy": strategy,
                "tokens_before": tokens_before,
                "tokens_after": tokens_after,
                "summary_tokens": summary_tokens,
                "created_at": created_at,
            }
        )
        if instruction is not UNSET:
            field_dict["instruction"] = instruction
        if model is not UNSET:
            field_dict["model"] = model
        if tool_results_cleared is not UNSET:
            field_dict["tool_results_cleared"] = tool_results_cleared
        if exact_tokens is not UNSET:
            field_dict["exact_tokens"] = exact_tokens
        if previous_id is not UNSET:
            field_dict["previous_id"] = previous_id
        if undone is not UNSET:
            field_dict["undone"] = undone
        if mid_turn is not UNSET:
            field_dict["mid_turn"] = mid_turn
        if pinned_id is not UNSET:
            field_dict["pinned_id"] = pinned_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        version = d.pop("version")

        boundary_id = d.pop("boundary_id")

        covered = d.pop("covered")

        turns = d.pop("turns")

        kept_turns = d.pop("kept_turns")

        trigger = check_compaction_trigger(d.pop("trigger"))

        strategy = check_compaction_strategy(d.pop("strategy"))

        tokens_before = d.pop("tokens_before")

        tokens_after = d.pop("tokens_after")

        summary_tokens = d.pop("summary_tokens")

        created_at = d.pop("created_at")

        def _parse_instruction(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        instruction = _parse_instruction(d.pop("instruction", UNSET))

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        tool_results_cleared = d.pop("tool_results_cleared", UNSET)

        exact_tokens = d.pop("exact_tokens", UNSET)

        def _parse_previous_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        previous_id = _parse_previous_id(d.pop("previous_id", UNSET))

        undone = d.pop("undone", UNSET)

        mid_turn = d.pop("mid_turn", UNSET)

        def _parse_pinned_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        pinned_id = _parse_pinned_id(d.pop("pinned_id", UNSET))

        compaction_marker = cls(
            version=version,
            boundary_id=boundary_id,
            covered=covered,
            turns=turns,
            kept_turns=kept_turns,
            trigger=trigger,
            strategy=strategy,
            tokens_before=tokens_before,
            tokens_after=tokens_after,
            summary_tokens=summary_tokens,
            created_at=created_at,
            instruction=instruction,
            model=model,
            tool_results_cleared=tool_results_cleared,
            exact_tokens=exact_tokens,
            previous_id=previous_id,
            undone=undone,
            mid_turn=mid_turn,
            pinned_id=pinned_id,
        )

        compaction_marker.additional_properties = d
        return compaction_marker

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
