from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.server_status_type_0 import ServerStatusType0, check_server_status_type_0
from ..models.server_status_type_1 import ServerStatusType1, check_server_status_type_1
from ..models.server_status_type_2 import ServerStatusType2, check_server_status_type_2
from ..models.server_status_type_3 import ServerStatusType3, check_server_status_type_3
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.active_server_info import ActiveServerInfo
    from ..models.profile_view import ProfileView
    from ..models.restart_state import RestartState
    from ..models.server_failure import ServerFailure
    from ..models.server_status_type_4 import ServerStatusType4


T = TypeVar("T", bound="ServerView")


@_attrs_define
class ServerView:
    """A server: a running/known instance, a saved profile, or both (same id).

    Attributes:
        id (str):
        name (str): Profile name (instances without a profile: the id).
        is_profile (bool): A saved profile exists with this id.
        status (ServerStatusType0 | ServerStatusType1 | ServerStatusType2 | ServerStatusType3 | ServerStatusType4):
        active (ActiveServerInfo | None | Unset):
        endpoint (None | str | Unset): OpenAI-compatible base URL while starting/running (`http://host:port/v1`).
        served_alias (None | str | Unset): Model id the server serves (`/v1/models`): definition alias or file stem.
        definition_id (None | str | Unset):
        profile (None | ProfileView | Unset):
        failure (None | ServerFailure | Unset):
        restart (None | RestartState | Unset):
    """

    id: str
    name: str
    is_profile: bool
    status: (
        ServerStatusType0
        | ServerStatusType1
        | ServerStatusType2
        | ServerStatusType3
        | ServerStatusType4
    )
    active: ActiveServerInfo | None | Unset = UNSET
    endpoint: None | str | Unset = UNSET
    served_alias: None | str | Unset = UNSET
    definition_id: None | str | Unset = UNSET
    profile: None | ProfileView | Unset = UNSET
    failure: None | ServerFailure | Unset = UNSET
    restart: None | RestartState | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.active_server_info import ActiveServerInfo  # noqa: PLC0415
        from ..models.profile_view import ProfileView  # noqa: PLC0415
        from ..models.restart_state import RestartState  # noqa: PLC0415
        from ..models.server_failure import ServerFailure  # noqa: PLC0415

        id = self.id

        name = self.name

        is_profile = self.is_profile

        status: dict[str, Any] | str
        if isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        elif isinstance(self.status, str):
            status = self.status
        else:
            status = self.status.to_dict()

        active: dict[str, Any] | None | Unset
        if isinstance(self.active, Unset):
            active = UNSET
        elif isinstance(self.active, ActiveServerInfo):
            active = self.active.to_dict()
        else:
            active = self.active

        endpoint: None | str | Unset
        if isinstance(self.endpoint, Unset):
            endpoint = UNSET
        else:
            endpoint = self.endpoint

        served_alias: None | str | Unset
        if isinstance(self.served_alias, Unset):
            served_alias = UNSET
        else:
            served_alias = self.served_alias

        definition_id: None | str | Unset
        if isinstance(self.definition_id, Unset):
            definition_id = UNSET
        else:
            definition_id = self.definition_id

        profile: dict[str, Any] | None | Unset
        if isinstance(self.profile, Unset):
            profile = UNSET
        elif isinstance(self.profile, ProfileView):
            profile = self.profile.to_dict()
        else:
            profile = self.profile

        failure: dict[str, Any] | None | Unset
        if isinstance(self.failure, Unset):
            failure = UNSET
        elif isinstance(self.failure, ServerFailure):
            failure = self.failure.to_dict()
        else:
            failure = self.failure

        restart: dict[str, Any] | None | Unset
        if isinstance(self.restart, Unset):
            restart = UNSET
        elif isinstance(self.restart, RestartState):
            restart = self.restart.to_dict()
        else:
            restart = self.restart

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "is_profile": is_profile,
                "status": status,
            }
        )
        if active is not UNSET:
            field_dict["active"] = active
        if endpoint is not UNSET:
            field_dict["endpoint"] = endpoint
        if served_alias is not UNSET:
            field_dict["served_alias"] = served_alias
        if definition_id is not UNSET:
            field_dict["definition_id"] = definition_id
        if profile is not UNSET:
            field_dict["profile"] = profile
        if failure is not UNSET:
            field_dict["failure"] = failure
        if restart is not UNSET:
            field_dict["restart"] = restart

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.active_server_info import ActiveServerInfo  # noqa: PLC0415
        from ..models.profile_view import ProfileView  # noqa: PLC0415
        from ..models.restart_state import RestartState  # noqa: PLC0415
        from ..models.server_failure import ServerFailure  # noqa: PLC0415
        from ..models.server_status_type_4 import ServerStatusType4  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        is_profile = d.pop("is_profile")

        def _parse_status(
            data: object,
        ) -> (
            ServerStatusType0
            | ServerStatusType1
            | ServerStatusType2
            | ServerStatusType3
            | ServerStatusType4
        ):
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_0 = check_server_status_type_0(data)

                return componentsschemas_server_status_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_1 = check_server_status_type_1(data)

                return componentsschemas_server_status_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_2 = check_server_status_type_2(data)

                return componentsschemas_server_status_type_2
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_server_status_type_3 = check_server_status_type_3(data)

                return componentsschemas_server_status_type_3
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_server_status_type_4 = ServerStatusType4.from_dict(data)

            return componentsschemas_server_status_type_4

        status = _parse_status(d.pop("status"))

        def _parse_active(data: object) -> ActiveServerInfo | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                active_type_1 = ActiveServerInfo.from_dict(data)

                return active_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(ActiveServerInfo | None | Unset, data)

        active = _parse_active(d.pop("active", UNSET))

        def _parse_endpoint(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        endpoint = _parse_endpoint(d.pop("endpoint", UNSET))

        def _parse_served_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        served_alias = _parse_served_alias(d.pop("served_alias", UNSET))

        def _parse_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        definition_id = _parse_definition_id(d.pop("definition_id", UNSET))

        def _parse_profile(data: object) -> None | ProfileView | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                profile_type_1 = ProfileView.from_dict(data)

                return profile_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ProfileView | Unset, data)

        profile = _parse_profile(d.pop("profile", UNSET))

        def _parse_failure(data: object) -> None | ServerFailure | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                failure_type_1 = ServerFailure.from_dict(data)

                return failure_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ServerFailure | Unset, data)

        failure = _parse_failure(d.pop("failure", UNSET))

        def _parse_restart(data: object) -> None | RestartState | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                restart_type_1 = RestartState.from_dict(data)

                return restart_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | RestartState | Unset, data)

        restart = _parse_restart(d.pop("restart", UNSET))

        server_view = cls(
            id=id,
            name=name,
            is_profile=is_profile,
            status=status,
            active=active,
            endpoint=endpoint,
            served_alias=served_alias,
            definition_id=definition_id,
            profile=profile,
            failure=failure,
            restart=restart,
        )

        server_view.additional_properties = d
        return server_view

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
