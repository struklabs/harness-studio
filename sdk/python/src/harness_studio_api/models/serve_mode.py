from typing import Literal

ServeMode = Literal["chat", "embedding", "image", "pipeline", "rerank", "transcription"]

SERVE_MODE_VALUES: set[ServeMode] = {
    "chat",
    "embedding",
    "image",
    "pipeline",
    "rerank",
    "transcription",
}


def check_serve_mode(value: str) -> ServeMode:
    if value in SERVE_MODE_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {SERVE_MODE_VALUES!r}")
