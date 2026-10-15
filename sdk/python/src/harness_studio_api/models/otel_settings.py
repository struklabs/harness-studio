from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.otel_protocol import OtelProtocol, check_otel_protocol
from ..types import UNSET, Unset

T = TypeVar("T", bound="OtelSettings")


@_attrs_define
class OtelSettings:
    """`settings.json` → `otel`. Changed only through [`save_settings`] (the general settings
    save keeps the stored block, like `gateway` and `control_api`).

        Attributes:
            enabled (bool | Unset): Off by default. Default: False.
            endpoint (str | Unset): Collector base URL (`http://host:4318` for HTTP, `http://host:4317` for gRPC). For HTTP
                the signal paths `/v1/traces`, `/v1/logs` and `/v1/metrics` are appended. Default: 'http://127.0.0.1:4318'.
            protocol (OtelProtocol | Unset): Wire protocol of the OTLP endpoint.
            service_name (str | Unset): `service.name` resource attribute. Default: 'harness-studio'.
            traces (bool | Unset):  Default: True.
            logs (bool | Unset): Gateway request log records. Off by default, independently of traces. Default: False.
            metrics (bool | Unset):  Default: True.
            metrics_interval_secs (int | Unset): Seconds between metric exports (10..=3600). Default: 60.
            include_content (bool | Unset): Put prompts and answers on spans, only for model definitions that capture
                content. Default: False.
            header_names (list[str] | Unset): Names of the extra export headers; the values live in the OS keychain.
    """

    enabled: bool | Unset = False
    endpoint: str | Unset = "http://127.0.0.1:4318"
    protocol: OtelProtocol | Unset = UNSET
    service_name: str | Unset = "harness-studio"
    traces: bool | Unset = True
    logs: bool | Unset = False
    metrics: bool | Unset = True
    metrics_interval_secs: int | Unset = 60
    include_content: bool | Unset = False
    header_names: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        enabled = self.enabled

        endpoint = self.endpoint

        protocol: str | Unset = UNSET
        if not isinstance(self.protocol, Unset):
            protocol = self.protocol

        service_name = self.service_name

        traces = self.traces

        logs = self.logs

        metrics = self.metrics

        metrics_interval_secs = self.metrics_interval_secs

        include_content = self.include_content

        header_names: list[str] | Unset = UNSET
        if not isinstance(self.header_names, Unset):
            header_names = self.header_names

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if enabled is not UNSET:
            field_dict["enabled"] = enabled
        if endpoint is not UNSET:
            field_dict["endpoint"] = endpoint
        if protocol is not UNSET:
            field_dict["protocol"] = protocol
        if service_name is not UNSET:
            field_dict["service_name"] = service_name
        if traces is not UNSET:
            field_dict["traces"] = traces
        if logs is not UNSET:
            field_dict["logs"] = logs
        if metrics is not UNSET:
            field_dict["metrics"] = metrics
        if metrics_interval_secs is not UNSET:
            field_dict["metrics_interval_secs"] = metrics_interval_secs
        if include_content is not UNSET:
            field_dict["include_content"] = include_content
        if header_names is not UNSET:
            field_dict["header_names"] = header_names

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        enabled = d.pop("enabled", UNSET)

        endpoint = d.pop("endpoint", UNSET)

        _protocol = d.pop("protocol", UNSET)
        protocol: OtelProtocol | Unset
        if isinstance(_protocol, Unset):
            protocol = UNSET
        else:
            protocol = check_otel_protocol(_protocol)

        service_name = d.pop("service_name", UNSET)

        traces = d.pop("traces", UNSET)

        logs = d.pop("logs", UNSET)

        metrics = d.pop("metrics", UNSET)

        metrics_interval_secs = d.pop("metrics_interval_secs", UNSET)

        include_content = d.pop("include_content", UNSET)

        header_names = cast(list[str], d.pop("header_names", UNSET))

        otel_settings = cls(
            enabled=enabled,
            endpoint=endpoint,
            protocol=protocol,
            service_name=service_name,
            traces=traces,
            logs=logs,
            metrics=metrics,
            metrics_interval_secs=metrics_interval_secs,
            include_content=include_content,
            header_names=header_names,
        )

        otel_settings.additional_properties = d
        return otel_settings

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
