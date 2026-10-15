from typing import Literal

GpuVendorType2 = Literal["Intel"]

GPU_VENDOR_TYPE_2_VALUES: set[GpuVendorType2] = {
    "Intel",
}


def check_gpu_vendor_type_2(value: str) -> GpuVendorType2:
    if value in GPU_VENDOR_TYPE_2_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {GPU_VENDOR_TYPE_2_VALUES!r}")
