from typing import Literal

GpuVendorType1 = Literal["Amd"]

GPU_VENDOR_TYPE_1_VALUES: set[GpuVendorType1] = {
    "Amd",
}


def check_gpu_vendor_type_1(value: str) -> GpuVendorType1:
    if value in GPU_VENDOR_TYPE_1_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {GPU_VENDOR_TYPE_1_VALUES!r}")
