from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.failure_kind import FailureKind, check_failure_kind
from ..models.failure_phase import FailurePhase, check_failure_phase
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.server_failure_params import ServerFailureParams


T = TypeVar("T", bound="ServerFailure")


@_attrs_define
class ServerFailure:
    """A classified failure of one run.

    Attributes:
        code (str): Stable machine code (`servers.out_of_memory`, `servers.port_in_use`...).
        kind (FailureKind): Failure classes, most specific first (the order [`classify`] tries them).
        message_key (str): i18n key of the message (`failure.out_of_memory`...).
        params (ServerFailureParams): Values for the message (`flag`, `arch`, `port`, `library`, `exit`, `seconds`...).
        phase (FailurePhase): When the run failed.
        evidence (list[str]): Log lines the classification is based on (at most 4).
        at (int): Unix seconds.
        exit_code (int | None | Unset): Process exit code (Windows NTSTATUS values are negative here; `params.exit` has
            hex).
    """

    code: str
    kind: FailureKind
    message_key: str
    params: ServerFailureParams
    phase: FailurePhase
    evidence: list[str]
    at: int
    exit_code: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code = self.code

        kind: str = self.kind

        message_key = self.message_key

        params = self.params.to_dict()

        phase: str = self.phase

        evidence = self.evidence

        at = self.at

        exit_code: int | None | Unset
        if isinstance(self.exit_code, Unset):
            exit_code = UNSET
        else:
            exit_code = self.exit_code

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code": code,
                "kind": kind,
                "message_key": message_key,
                "params": params,
                "phase": phase,
                "evidence": evidence,
                "at": at,
            }
        )
        if exit_code is not UNSET:
            field_dict["exit_code"] = exit_code

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.server_failure_params import ServerFailureParams  # noqa: PLC0415

        d = dict(src_dict)
        code = d.pop("code")

        kind = check_failure_kind(d.pop("kind"))

        message_key = d.pop("message_key")

        params = ServerFailureParams.from_dict(d.pop("params"))

        phase = check_failure_phase(d.pop("phase"))

        evidence = cast(list[str], d.pop("evidence"))

        at = d.pop("at")

        def _parse_exit_code(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        exit_code = _parse_exit_code(d.pop("exit_code", UNSET))

        server_failure = cls(
            code=code,
            kind=kind,
            message_key=message_key,
            params=params,
            phase=phase,
            evidence=evidence,
            at=at,
            exit_code=exit_code,
        )

        server_failure.additional_properties = d
        return server_failure

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
