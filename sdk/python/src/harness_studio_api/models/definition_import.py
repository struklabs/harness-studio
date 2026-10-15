from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.inference_preset import InferencePreset
    from ..models.model_definition import ModelDefinition


T = TypeVar("T", bound="DefinitionImport")


@_attrs_define
class DefinitionImport:
    """What an import found (nothing is saved yet).

    Attributes:
        definition (ModelDefinition): A logical model: what clients ask for by `alias`.
        model_found (bool): The base model file exists on this machine.
        missing (list[str]): Other referenced files (mmproj, adapters, draft) that were not found.
        alias_taken (bool): The file's alias is already used here.
        suggested_alias (str): Free alias to use (`<alias>-2`... when taken).
        hf_ref (None | str | Unset): Hugging Face reference of the model (to download it when it's missing).
        preset (InferencePreset | None | Unset):
        chat_template_text (None | str | Unset): Embedded chat template text; the import saves it as a template file and
            points
            `load.chat_template_file` at it.
    """

    definition: ModelDefinition
    model_found: bool
    missing: list[str]
    alias_taken: bool
    suggested_alias: str
    hf_ref: None | str | Unset = UNSET
    preset: InferencePreset | None | Unset = UNSET
    chat_template_text: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.inference_preset import InferencePreset  # noqa: PLC0415

        definition = self.definition.to_dict()

        model_found = self.model_found

        missing = self.missing

        alias_taken = self.alias_taken

        suggested_alias = self.suggested_alias

        hf_ref: None | str | Unset
        if isinstance(self.hf_ref, Unset):
            hf_ref = UNSET
        else:
            hf_ref = self.hf_ref

        preset: dict[str, Any] | None | Unset
        if isinstance(self.preset, Unset):
            preset = UNSET
        elif isinstance(self.preset, InferencePreset):
            preset = self.preset.to_dict()
        else:
            preset = self.preset

        chat_template_text: None | str | Unset
        if isinstance(self.chat_template_text, Unset):
            chat_template_text = UNSET
        else:
            chat_template_text = self.chat_template_text

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "definition": definition,
                "model_found": model_found,
                "missing": missing,
                "alias_taken": alias_taken,
                "suggested_alias": suggested_alias,
            }
        )
        if hf_ref is not UNSET:
            field_dict["hf_ref"] = hf_ref
        if preset is not UNSET:
            field_dict["preset"] = preset
        if chat_template_text is not UNSET:
            field_dict["chat_template_text"] = chat_template_text

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.inference_preset import InferencePreset  # noqa: PLC0415
        from ..models.model_definition import ModelDefinition  # noqa: PLC0415

        d = dict(src_dict)
        definition = ModelDefinition.from_dict(d.pop("definition"))

        model_found = d.pop("model_found")

        missing = cast(list[str], d.pop("missing"))

        alias_taken = d.pop("alias_taken")

        suggested_alias = d.pop("suggested_alias")

        def _parse_hf_ref(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        hf_ref = _parse_hf_ref(d.pop("hf_ref", UNSET))

        def _parse_preset(data: object) -> InferencePreset | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                preset_type_1 = InferencePreset.from_dict(data)

                return preset_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(InferencePreset | None | Unset, data)

        preset = _parse_preset(d.pop("preset", UNSET))

        def _parse_chat_template_text(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        chat_template_text = _parse_chat_template_text(d.pop("chat_template_text", UNSET))

        definition_import = cls(
            definition=definition,
            model_found=model_found,
            missing=missing,
            alias_taken=alias_taken,
            suggested_alias=suggested_alias,
            hf_ref=hf_ref,
            preset=preset,
            chat_template_text=chat_template_text,
        )

        definition_import.additional_properties = d
        return definition_import

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
