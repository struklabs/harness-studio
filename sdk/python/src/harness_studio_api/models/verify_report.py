from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="VerifyReport")


@_attrs_define
class VerifyReport:
    """Result of a full check of a runtime.

    Attributes:
        id (str):
        ok (bool): Files match the recorded hashes and `llama-server --version` runs.
        changed (list[str]): Recorded files whose SHA-256 differs now.
        missing (list[str]): Recorded files that are gone.
        added (list[str]): Code files that weren't there when the runtime was added.
        accepted (bool): The new hashes were accepted (local runtimes, `accept_changes`).
        version (None | str | Unset):
        error (None | str | Unset):
    """

    id: str
    ok: bool
    changed: list[str]
    missing: list[str]
    added: list[str]
    accepted: bool
    version: None | str | Unset = UNSET
    error: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        ok = self.ok

        changed = self.changed

        missing = self.missing

        added = self.added

        accepted = self.accepted

        version: None | str | Unset
        if isinstance(self.version, Unset):
            version = UNSET
        else:
            version = self.version

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "ok": ok,
                "changed": changed,
                "missing": missing,
                "added": added,
                "accepted": accepted,
            }
        )
        if version is not UNSET:
            field_dict["version"] = version
        if error is not UNSET:
            field_dict["error"] = error

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        ok = d.pop("ok")

        changed = cast(list[str], d.pop("changed"))

        missing = cast(list[str], d.pop("missing"))

        added = cast(list[str], d.pop("added"))

        accepted = d.pop("accepted")

        def _parse_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        version = _parse_version(d.pop("version", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        verify_report = cls(
            id=id,
            ok=ok,
            changed=changed,
            missing=missing,
            added=added,
            accepted=accepted,
            version=version,
            error=error,
        )

        verify_report.additional_properties = d
        return verify_report

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
