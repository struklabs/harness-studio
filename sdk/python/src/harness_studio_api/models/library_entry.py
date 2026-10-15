from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.local_model import LocalModel
    from ..models.model_caps import ModelCaps
    from ..models.model_record import ModelRecord
    from ..models.param_info import ParamInfo
    from ..models.repair_info import RepairInfo
    from ..models.shard_status import ShardStatus


T = TypeVar("T", bound="LibraryEntry")


@_attrs_define
class LibraryEntry:
    """One local model file (or split set) as the library shows it.

    Attributes:
        model (LocalModel):
        params (ParamInfo):
        caps (ModelCaps): What a model can do, from its header, template and companion files.
        record (ModelRecord): Capability, format and runtime verdict of one model (a local file or a Hub repo).
        pairs (list[str]): LLM: projector files that belong to it. Projector: the LLMs it belongs to (ids).
        multimodal (bool): LLM with a projector (or vision in a definition): "VLM / multimodal".
        favorite (bool):
        draft_for (list[str]): Bigger local LLMs this one can draft for (ids).
        shards (None | ShardStatus | Unset):
        repair (None | RepairInfo | Unset):
        repo_id (None | str | Unset): Hugging Face repo (`owner/name`) the file was downloaded from, read from its
            folder
            (`<library>/<owner>/<repo>/…`); the UI shows the owner's avatar.
        last_used_at (int | None | Unset):
        last_definition_id (None | str | Unset):
        preset_used (None | str | Unset): Preset used last: the newest conversation with one of its aliases, else the
            default
            preset of the definition that started it last.
        added_at (int | None | Unset): When the file arrived (Unix ms; download completion or copy time).
    """

    model: LocalModel
    params: ParamInfo
    caps: ModelCaps
    record: ModelRecord
    pairs: list[str]
    multimodal: bool
    favorite: bool
    draft_for: list[str]
    shards: None | ShardStatus | Unset = UNSET
    repair: None | RepairInfo | Unset = UNSET
    repo_id: None | str | Unset = UNSET
    last_used_at: int | None | Unset = UNSET
    last_definition_id: None | str | Unset = UNSET
    preset_used: None | str | Unset = UNSET
    added_at: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.repair_info import RepairInfo  # noqa: PLC0415
        from ..models.shard_status import ShardStatus  # noqa: PLC0415

        model = self.model.to_dict()

        params = self.params.to_dict()

        caps = self.caps.to_dict()

        record = self.record.to_dict()

        pairs = self.pairs

        multimodal = self.multimodal

        favorite = self.favorite

        draft_for = self.draft_for

        shards: dict[str, Any] | None | Unset
        if isinstance(self.shards, Unset):
            shards = UNSET
        elif isinstance(self.shards, ShardStatus):
            shards = self.shards.to_dict()
        else:
            shards = self.shards

        repair: dict[str, Any] | None | Unset
        if isinstance(self.repair, Unset):
            repair = UNSET
        elif isinstance(self.repair, RepairInfo):
            repair = self.repair.to_dict()
        else:
            repair = self.repair

        repo_id: None | str | Unset
        if isinstance(self.repo_id, Unset):
            repo_id = UNSET
        else:
            repo_id = self.repo_id

        last_used_at: int | None | Unset
        if isinstance(self.last_used_at, Unset):
            last_used_at = UNSET
        else:
            last_used_at = self.last_used_at

        last_definition_id: None | str | Unset
        if isinstance(self.last_definition_id, Unset):
            last_definition_id = UNSET
        else:
            last_definition_id = self.last_definition_id

        preset_used: None | str | Unset
        if isinstance(self.preset_used, Unset):
            preset_used = UNSET
        else:
            preset_used = self.preset_used

        added_at: int | None | Unset
        if isinstance(self.added_at, Unset):
            added_at = UNSET
        else:
            added_at = self.added_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "model": model,
                "params": params,
                "caps": caps,
                "record": record,
                "pairs": pairs,
                "multimodal": multimodal,
                "favorite": favorite,
                "draft_for": draft_for,
            }
        )
        if shards is not UNSET:
            field_dict["shards"] = shards
        if repair is not UNSET:
            field_dict["repair"] = repair
        if repo_id is not UNSET:
            field_dict["repo_id"] = repo_id
        if last_used_at is not UNSET:
            field_dict["last_used_at"] = last_used_at
        if last_definition_id is not UNSET:
            field_dict["last_definition_id"] = last_definition_id
        if preset_used is not UNSET:
            field_dict["preset_used"] = preset_used
        if added_at is not UNSET:
            field_dict["added_at"] = added_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.local_model import LocalModel  # noqa: PLC0415
        from ..models.model_caps import ModelCaps  # noqa: PLC0415
        from ..models.model_record import ModelRecord  # noqa: PLC0415
        from ..models.param_info import ParamInfo  # noqa: PLC0415
        from ..models.repair_info import RepairInfo  # noqa: PLC0415
        from ..models.shard_status import ShardStatus  # noqa: PLC0415

        d = dict(src_dict)
        model = LocalModel.from_dict(d.pop("model"))

        params = ParamInfo.from_dict(d.pop("params"))

        caps = ModelCaps.from_dict(d.pop("caps"))

        record = ModelRecord.from_dict(d.pop("record"))

        pairs = cast(list[str], d.pop("pairs"))

        multimodal = d.pop("multimodal")

        favorite = d.pop("favorite")

        draft_for = cast(list[str], d.pop("draft_for"))

        def _parse_shards(data: object) -> None | ShardStatus | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                shards_type_1 = ShardStatus.from_dict(data)

                return shards_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ShardStatus | Unset, data)

        shards = _parse_shards(d.pop("shards", UNSET))

        def _parse_repair(data: object) -> None | RepairInfo | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                repair_type_1 = RepairInfo.from_dict(data)

                return repair_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RepairInfo | Unset, data)

        repair = _parse_repair(d.pop("repair", UNSET))

        def _parse_repo_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        repo_id = _parse_repo_id(d.pop("repo_id", UNSET))

        def _parse_last_used_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        last_used_at = _parse_last_used_at(d.pop("last_used_at", UNSET))

        def _parse_last_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        last_definition_id = _parse_last_definition_id(d.pop("last_definition_id", UNSET))

        def _parse_preset_used(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        preset_used = _parse_preset_used(d.pop("preset_used", UNSET))

        def _parse_added_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        added_at = _parse_added_at(d.pop("added_at", UNSET))

        library_entry = cls(
            model=model,
            params=params,
            caps=caps,
            record=record,
            pairs=pairs,
            multimodal=multimodal,
            favorite=favorite,
            draft_for=draft_for,
            shards=shards,
            repair=repair,
            repo_id=repo_id,
            last_used_at=last_used_at,
            last_definition_id=last_definition_id,
            preset_used=preset_used,
            added_at=added_at,
        )

        library_entry.additional_properties = d
        return library_entry

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
