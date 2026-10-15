from typing import Literal

OrtVariant = Literal["cpu", "cuda", "directml"]

ORT_VARIANT_VALUES: set[OrtVariant] = {
    "cpu",
    "cuda",
    "directml",
}


def check_ort_variant(value: str) -> OrtVariant:
    if value in ORT_VARIANT_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {ORT_VARIANT_VALUES!r}")
