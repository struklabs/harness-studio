from typing import Literal

ModelRefType0Kind = Literal["local"]

MODEL_REF_TYPE_0_KIND_VALUES: set[ModelRefType0Kind] = {
    "local",
}


def check_model_ref_type_0_kind(value: str) -> ModelRefType0Kind:
    if value in MODEL_REF_TYPE_0_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MODEL_REF_TYPE_0_KIND_VALUES!r}")
