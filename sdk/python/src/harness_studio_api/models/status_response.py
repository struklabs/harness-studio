from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.gateway_status import GatewayStatus
    from ..models.hardware_summary import HardwareSummary
    from ..models.instance_summary import InstanceSummary
    from ..models.platform_support import PlatformSupport
    from ..models.runtime_summary import RuntimeSummary
    from ..models.server_counts import ServerCounts


T = TypeVar("T", bound="StatusResponse")


@_attrs_define
class StatusResponse:
    """
    Attributes:
        version (str):
        api_version (str): Control API version (`v1`).
        instance (InstanceSummary):
        runtime (RuntimeSummary): The llama-server binary the next start uses and what it can do.
        hardware (HardwareSummary):
        servers (ServerCounts):
        gateway (GatewayStatus): What the gateway listener is doing right now (process-wide; set by the HTTP side).
        onnx_runtime (PlatformSupport): [`supported`] with the OS and architecture it was decided for (the UI names
            them).
    """

    version: str
    api_version: str
    instance: InstanceSummary
    runtime: RuntimeSummary
    hardware: HardwareSummary
    servers: ServerCounts
    gateway: GatewayStatus
    onnx_runtime: PlatformSupport
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        version = self.version

        api_version = self.api_version

        instance = self.instance.to_dict()

        runtime = self.runtime.to_dict()

        hardware = self.hardware.to_dict()

        servers = self.servers.to_dict()

        gateway = self.gateway.to_dict()

        onnx_runtime = self.onnx_runtime.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "version": version,
                "api_version": api_version,
                "instance": instance,
                "runtime": runtime,
                "hardware": hardware,
                "servers": servers,
                "gateway": gateway,
                "onnx_runtime": onnx_runtime,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.gateway_status import GatewayStatus  # noqa: PLC0415
        from ..models.hardware_summary import HardwareSummary  # noqa: PLC0415
        from ..models.instance_summary import InstanceSummary  # noqa: PLC0415
        from ..models.platform_support import PlatformSupport  # noqa: PLC0415
        from ..models.runtime_summary import RuntimeSummary  # noqa: PLC0415
        from ..models.server_counts import ServerCounts  # noqa: PLC0415

        d = dict(src_dict)
        version = d.pop("version")

        api_version = d.pop("api_version")

        instance = InstanceSummary.from_dict(d.pop("instance"))

        runtime = RuntimeSummary.from_dict(d.pop("runtime"))

        hardware = HardwareSummary.from_dict(d.pop("hardware"))

        servers = ServerCounts.from_dict(d.pop("servers"))

        gateway = GatewayStatus.from_dict(d.pop("gateway"))

        onnx_runtime = PlatformSupport.from_dict(d.pop("onnx_runtime"))

        status_response = cls(
            version=version,
            api_version=api_version,
            instance=instance,
            runtime=runtime,
            hardware=hardware,
            servers=servers,
            gateway=gateway,
            onnx_runtime=onnx_runtime,
        )

        status_response.additional_properties = d
        return status_response

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
