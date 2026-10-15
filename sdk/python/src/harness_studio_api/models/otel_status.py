from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="OtelStatus")


@_attrs_define
class OtelStatus:
    """Exporter state for Settings and diagnostics.

    Attributes:
        built (bool): This build contains the exporter (cargo feature `otel`).
        active (bool): Export is running.
        spans_exported (int):
        spans_dropped (int): Spans dropped because the export queue was full.
        logs_exported (int):
        logs_dropped (int):
        export_errors (int):
        grpc (bool | Unset): This build can export over OTLP/gRPC (cargo feature `otel-grpc`); OTLP/HTTP is always
            there when `built`.
        traces_url (None | str | Unset):
        metrics_url (None | str | Unset):
        logs_url (None | str | Unset):
        last_error (None | str | Unset):
        last_export_ms (int | None | Unset): Unix ms of the last successful span export.
    """

    built: bool
    active: bool
    spans_exported: int
    spans_dropped: int
    logs_exported: int
    logs_dropped: int
    export_errors: int
    grpc: bool | Unset = UNSET
    traces_url: None | str | Unset = UNSET
    metrics_url: None | str | Unset = UNSET
    logs_url: None | str | Unset = UNSET
    last_error: None | str | Unset = UNSET
    last_export_ms: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        built = self.built

        active = self.active

        spans_exported = self.spans_exported

        spans_dropped = self.spans_dropped

        logs_exported = self.logs_exported

        logs_dropped = self.logs_dropped

        export_errors = self.export_errors

        grpc = self.grpc

        traces_url: None | str | Unset
        if isinstance(self.traces_url, Unset):
            traces_url = UNSET
        else:
            traces_url = self.traces_url

        metrics_url: None | str | Unset
        if isinstance(self.metrics_url, Unset):
            metrics_url = UNSET
        else:
            metrics_url = self.metrics_url

        logs_url: None | str | Unset
        if isinstance(self.logs_url, Unset):
            logs_url = UNSET
        else:
            logs_url = self.logs_url

        last_error: None | str | Unset
        if isinstance(self.last_error, Unset):
            last_error = UNSET
        else:
            last_error = self.last_error

        last_export_ms: int | None | Unset
        if isinstance(self.last_export_ms, Unset):
            last_export_ms = UNSET
        else:
            last_export_ms = self.last_export_ms

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "built": built,
                "active": active,
                "spans_exported": spans_exported,
                "spans_dropped": spans_dropped,
                "logs_exported": logs_exported,
                "logs_dropped": logs_dropped,
                "export_errors": export_errors,
            }
        )
        if grpc is not UNSET:
            field_dict["grpc"] = grpc
        if traces_url is not UNSET:
            field_dict["traces_url"] = traces_url
        if metrics_url is not UNSET:
            field_dict["metrics_url"] = metrics_url
        if logs_url is not UNSET:
            field_dict["logs_url"] = logs_url
        if last_error is not UNSET:
            field_dict["last_error"] = last_error
        if last_export_ms is not UNSET:
            field_dict["last_export_ms"] = last_export_ms

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        built = d.pop("built")

        active = d.pop("active")

        spans_exported = d.pop("spans_exported")

        spans_dropped = d.pop("spans_dropped")

        logs_exported = d.pop("logs_exported")

        logs_dropped = d.pop("logs_dropped")

        export_errors = d.pop("export_errors")

        grpc = d.pop("grpc", UNSET)

        def _parse_traces_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        traces_url = _parse_traces_url(d.pop("traces_url", UNSET))

        def _parse_metrics_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        metrics_url = _parse_metrics_url(d.pop("metrics_url", UNSET))

        def _parse_logs_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        logs_url = _parse_logs_url(d.pop("logs_url", UNSET))

        def _parse_last_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        last_error = _parse_last_error(d.pop("last_error", UNSET))

        def _parse_last_export_ms(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        last_export_ms = _parse_last_export_ms(d.pop("last_export_ms", UNSET))

        otel_status = cls(
            built=built,
            active=active,
            spans_exported=spans_exported,
            spans_dropped=spans_dropped,
            logs_exported=logs_exported,
            logs_dropped=logs_dropped,
            export_errors=export_errors,
            grpc=grpc,
            traces_url=traces_url,
            metrics_url=metrics_url,
            logs_url=logs_url,
            last_error=last_error,
            last_export_ms=last_export_ms,
        )

        otel_status.additional_properties = d
        return otel_status

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
