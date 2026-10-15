from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.otel_protocol import OtelProtocol, check_otel_protocol
from ..types import UNSET, Unset

T = TypeVar("T", bound="TestExportResult")


@_attrs_define
class TestExportResult:
    """Result of "Test export": one real span sent to the configured endpoint.

    Attributes:
        ok (bool):
        url (str): Where the span went.
        protocol (OtelProtocol): Wire protocol of the OTLP endpoint.
        elapsed_ms (int):
        trace_id (str): Trace id of the test span (search for it in the backend).
        error (None | str | Unset):
    """

    ok: bool
    url: str
    protocol: OtelProtocol
    elapsed_ms: int
    trace_id: str
    error: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        ok = self.ok

        url = self.url

        protocol: str = self.protocol

        elapsed_ms = self.elapsed_ms

        trace_id = self.trace_id

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "ok": ok,
                "url": url,
                "protocol": protocol,
                "elapsed_ms": elapsed_ms,
                "trace_id": trace_id,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        ok = d.pop("ok")

        url = d.pop("url")

        protocol = check_otel_protocol(d.pop("protocol"))

        elapsed_ms = d.pop("elapsed_ms")

        trace_id = d.pop("trace_id")

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        test_export_result = cls(
            ok=ok,
            url=url,
            protocol=protocol,
            elapsed_ms=elapsed_ms,
            trace_id=trace_id,
            error=error,
        )

        test_export_result.additional_properties = d
        return test_export_result

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
