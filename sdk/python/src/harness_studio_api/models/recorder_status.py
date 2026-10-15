from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RecorderStatus")


@_attrs_define
class RecorderStatus:
    """Counters of the recorder since this process started.

    Attributes:
        running (bool): The writer thread has started (after the first request).
        written (int):
        dropped (int): Dropped because the queue was full.
        skipped (int): Skipped because recording is turned off.
        write_errors (int):
        last_error (None | str | Unset):
    """

    running: bool
    written: int
    dropped: int
    skipped: int
    write_errors: int
    last_error: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        running = self.running

        written = self.written

        dropped = self.dropped

        skipped = self.skipped

        write_errors = self.write_errors

        last_error: None | str | Unset
        if isinstance(self.last_error, Unset):
            last_error = UNSET
        else:
            last_error = self.last_error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "running": running,
                "written": written,
                "dropped": dropped,
                "skipped": skipped,
                "write_errors": write_errors,
            }
        )
        if last_error is not UNSET:
            field_dict["last_error"] = last_error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        running = d.pop("running")

        written = d.pop("written")

        dropped = d.pop("dropped")

        skipped = d.pop("skipped")

        write_errors = d.pop("write_errors")

        def _parse_last_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        last_error = _parse_last_error(d.pop("last_error", UNSET))

        recorder_status = cls(
            running=running,
            written=written,
            dropped=dropped,
            skipped=skipped,
            write_errors=write_errors,
            last_error=last_error,
        )

        recorder_status.additional_properties = d
        return recorder_status

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
