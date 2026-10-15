from typing import Literal

Capability = Literal[
    "asr", "audio", "chat", "embedding", "image_generation", "rerank", "tts", "vision"
]

CAPABILITY_VALUES: set[Capability] = {
    "asr",
    "audio",
    "chat",
    "embedding",
    "image_generation",
    "rerank",
    "tts",
    "vision",
}


def check_capability(value: str) -> Capability:
    if value in CAPABILITY_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {CAPABILITY_VALUES!r}")
