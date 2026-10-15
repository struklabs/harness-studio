from typing import Literal

GatewayEndpoint = Literal[
    "audio",
    "chat",
    "embeddings",
    "images",
    "messages",
    "metrics",
    "models",
    "ollama",
    "pipelines",
    "rerank",
    "videos",
]

GATEWAY_ENDPOINT_VALUES: set[GatewayEndpoint] = {
    "audio",
    "chat",
    "embeddings",
    "images",
    "messages",
    "metrics",
    "models",
    "ollama",
    "pipelines",
    "rerank",
    "videos",
}


def check_gateway_endpoint(value: str) -> GatewayEndpoint:
    if value in GATEWAY_ENDPOINT_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {GATEWAY_ENDPOINT_VALUES!r}")
