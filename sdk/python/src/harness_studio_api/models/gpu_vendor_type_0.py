from typing import Literal

GpuVendorType0 = Literal["Nvidia"]

GPU_VENDOR_TYPE_0_VALUES: set[GpuVendorType0] = {
    "Nvidia",
}


def check_gpu_vendor_type_0(value: str) -> GpuVendorType0:
    if value in GPU_VENDOR_TYPE_0_VALUES:
        return value
    raise TypeError(f"Unexpected value {value!r}. Expected one of {GPU_VENDOR_TYPE_0_VALUES!r}")
