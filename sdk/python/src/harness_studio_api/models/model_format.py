from typing import Literal

ModelFormat = Literal[
    "core_ml",
    "ctranslate2",
    "diffusers",
    "gguf",
    "mlx",
    "onnx",
    "pytorch",
    "safetensors",
    "unknown",
    "whisper_ggml",
]

MODEL_FORMAT_VALUES: set[ModelFormat] = {
    "core_ml",
    "ctranslate2",
    "diffusers",
    "gguf",
    "mlx",
    "onnx",
    "pytorch",
    "safetensors",
    "unknown",
    "whisper_ggml",
}


def check_model_format(value: str) -> ModelFormat:
    if value in MODEL_FORMAT_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MODEL_FORMAT_VALUES!r}")
