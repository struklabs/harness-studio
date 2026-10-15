from typing import Literal

ModelRefType1Kind = Literal["hf"]

MODEL_REF_TYPE_1_KIND_VALUES: set[ModelRefType1Kind] = {
    "hf",
}


def check_model_ref_type_1_kind(value: str) -> ModelRefType1Kind:
    if value in MODEL_REF_TYPE_1_KIND_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {MODEL_REF_TYPE_1_KIND_VALUES!r}")
