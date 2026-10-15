from typing import Literal

GpuVendorType3 = Literal["Apple"]

GPU_VENDOR_TYPE_3_VALUES: set[GpuVendorType3] = {
    "Apple",
}


def check_gpu_vendor_type_3(value: str) -> GpuVendorType3:
    if value in GPU_VENDOR_TYPE_3_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {GPU_VENDOR_TYPE_3_VALUES!r}")
