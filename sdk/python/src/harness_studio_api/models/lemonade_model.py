from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.model_kind import ModelKind, check_model_kind

T = TypeVar("T", bound="LemonadeModel")


@_attrs_define
class LemonadeModel:
    """One model of Lemonade's registry (`GET /v1/models?show_all=true`).

    Attributes:
        id (str): The name clients use (`Qwen3-0.6B-GGUF`, `user.my-model`).
        recipe (str): `llamacpp`, `whispercpp`, `flm`, `ryzenai-llm`, `sd-cpp`, `kokoro`...
        checkpoint (str): Hugging Face reference (`unsloth/Qwen3-0.6B-GGUF:Q4_0`).
        labels (list[str]):
        kind (ModelKind): What a GGUF file is. Decided from its header by [`detect_kind`].
        size_gb (float): Download size in GB, as Lemonade lists it.
        downloaded (bool):
        suggested (bool):
        update_available (bool):
    """

    id: str
    recipe: str
    checkpoint: str
    labels: list[str]
    kind: ModelKind
    size_gb: float
    downloaded: bool
    suggested: bool
    update_available: bool
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        recipe = self.recipe

        checkpoint = self.checkpoint

        labels = self.labels

        kind: str = self.kind

        size_gb = self.size_gb

        downloaded = self.downloaded

        suggested = self.suggested

        update_available = self.update_available

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "recipe": recipe,
                "checkpoint": checkpoint,
                "labels": labels,
                "kind": kind,
                "size_gb": size_gb,
                "downloaded": downloaded,
                "suggested": suggested,
                "update_available": update_available,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        recipe = d.pop("recipe")

        checkpoint = d.pop("checkpoint")

        labels = cast(list[str], d.pop("labels"))

        kind = check_model_kind(d.pop("kind"))

        size_gb = d.pop("size_gb")

        downloaded = d.pop("downloaded")

        suggested = d.pop("suggested")

        update_available = d.pop("update_available")

        lemonade_model = cls(
            id=id,
            recipe=recipe,
            checkpoint=checkpoint,
            labels=labels,
            kind=kind,
            size_gb=size_gb,
            downloaded=downloaded,
            suggested=suggested,
            update_available=update_available,
        )

        lemonade_model.additional_properties = d
        return lemonade_model

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
