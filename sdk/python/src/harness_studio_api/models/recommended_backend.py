from typing import Literal

RecommendedBackend = Literal["Cpu", "Cuda12", "Metal", "Rocm", "Sycl", "Vulkan"]

RECOMMENDED_BACKEND_VALUES: set[RecommendedBackend] = {
    "Cpu",
    "Cuda12",
    "Metal",
    "Rocm",
    "Sycl",
    "Vulkan",
}


def check_recommended_backend(value: str) -> RecommendedBackend:
    if value in RECOMMENDED_BACKEND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {RECOMMENDED_BACKEND_VALUES!r}")
