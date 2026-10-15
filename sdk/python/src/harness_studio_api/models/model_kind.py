from typing import Literal

ModelKind = Literal[
    "adapter",
    "asr",
    "embedding",
    "image",
    "llm",
    "mmproj",
    "pipeline",
    "reranker",
    "tts",
    "unknown",
]

MODEL_KIND_VALUES: set[ModelKind] = {
    "adapter",
    "asr",
    "embedding",
    "image",
    "llm",
    "mmproj",
    "pipeline",
    "reranker",
    "tts",
    "unknown",
}


def check_model_kind(value: str) -> ModelKind:
    if value in MODEL_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MODEL_KIND_VALUES!r}")
