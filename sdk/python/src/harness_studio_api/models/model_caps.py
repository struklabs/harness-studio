from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.reasoning_control import ReasoningControl
    from ..models.verified_caps import VerifiedCaps


T = TypeVar("T", bound="ModelCaps")


@_attrs_define
class ModelCaps:
    """What a model can do, from its header, template and companion files.

    Attributes:
        chat (bool): Has a chat template (text generation).
        tools (bool): The chat template handles `tools`.
        reasoning (bool): The chat template can reason (see `gguf::template_supports_reasoning`).
        vision (bool): A vision projector belongs to it (matched file, linked projector or a definition).
        audio (bool): Its projector has an audio encoder.
        embedding (bool):
        rerank (bool):
        reasoning_control (ReasoningControl | Unset):
        audio_input (bool | Unset): Audio content parts accepted by the model, independent of vision.
        asr (bool | Unset): Speech recognition (Whisper, sherpa-onnx or an ONNX CTC package).
        image (bool | Unset): Image / video generation (a stable-diffusion.cpp model).
        tts (bool | Unset): Speech synthesis (a Kokoro or Piper folder run by sherpa-onnx).
        verified (None | Unset | VerifiedCaps):
    """

    chat: bool
    tools: bool
    reasoning: bool
    vision: bool
    audio: bool
    embedding: bool
    rerank: bool
    reasoning_control: ReasoningControl | Unset = UNSET
    audio_input: bool | Unset = UNSET
    asr: bool | Unset = UNSET
    image: bool | Unset = UNSET
    tts: bool | Unset = UNSET
    verified: None | Unset | VerifiedCaps = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.verified_caps import VerifiedCaps  # noqa: PLC0415

        chat = self.chat

        tools = self.tools

        reasoning = self.reasoning

        vision = self.vision

        audio = self.audio

        embedding = self.embedding

        rerank = self.rerank

        reasoning_control: dict[str, Any] | Unset = UNSET
        if not isinstance(self.reasoning_control, Unset):
            reasoning_control = self.reasoning_control.to_dict()

        audio_input = self.audio_input

        asr = self.asr

        image = self.image

        tts = self.tts

        verified: dict[str, Any] | None | Unset
        if isinstance(self.verified, Unset):
            verified = UNSET
        elif isinstance(self.verified, VerifiedCaps):
            verified = self.verified.to_dict()
        else:
            verified = self.verified

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "chat": chat,
                "tools": tools,
                "reasoning": reasoning,
                "vision": vision,
                "audio": audio,
                "embedding": embedding,
                "rerank": rerank,
            }
        )
        if reasoning_control is not UNSET:
            field_dict["reasoning_control"] = reasoning_control
        if audio_input is not UNSET:
            field_dict["audio_input"] = audio_input
        if asr is not UNSET:
            field_dict["asr"] = asr
        if image is not UNSET:
            field_dict["image"] = image
        if tts is not UNSET:
            field_dict["tts"] = tts
        if verified is not UNSET:
            field_dict["verified"] = verified

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.reasoning_control import ReasoningControl  # noqa: PLC0415
        from ..models.verified_caps import VerifiedCaps  # noqa: PLC0415

        d = dict(src_dict)
        chat = d.pop("chat")

        tools = d.pop("tools")

        reasoning = d.pop("reasoning")

        vision = d.pop("vision")

        audio = d.pop("audio")

        embedding = d.pop("embedding")

        rerank = d.pop("rerank")

        _reasoning_control = d.pop("reasoning_control", UNSET)
        reasoning_control: ReasoningControl | Unset
        if isinstance(_reasoning_control, Unset):
            reasoning_control = UNSET
        else:
            reasoning_control = ReasoningControl.from_dict(_reasoning_control)

        audio_input = d.pop("audio_input", UNSET)

        asr = d.pop("asr", UNSET)

        image = d.pop("image", UNSET)

        tts = d.pop("tts", UNSET)

        def _parse_verified(data: object) -> None | Unset | VerifiedCaps:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                verified_type_1 = VerifiedCaps.from_dict(data)

                return verified_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | VerifiedCaps, data)

        verified = _parse_verified(d.pop("verified", UNSET))

        model_caps = cls(
            chat=chat,
            tools=tools,
            reasoning=reasoning,
            vision=vision,
            audio=audio,
            embedding=embedding,
            rerank=rerank,
            reasoning_control=reasoning_control,
            audio_input=audio_input,
            asr=asr,
            image=image,
            tts=tts,
            verified=verified,
        )

        model_caps.additional_properties = d
        return model_caps

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
