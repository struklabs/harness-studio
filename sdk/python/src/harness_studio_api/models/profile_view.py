from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.launch_config import LaunchConfig


T = TypeVar("T", bound="ProfileView")


@_attrs_define
class ProfileView:
    """A server profile in the shape the GUI uses (`list_server_profiles` & co.): the effective
    launch config of a `domain::ServerProfile` and its `ModelDefinition`.

        Attributes:
            id (str): Also used as the server instance id when the profile is started.
            name (str):
            config (LaunchConfig):
            model_id (None | str | Unset): Model id (= launch path) the profile serves; informational.
            description (str | Unset): Short free-text purpose ("OpenCode", "Open Design"...); shown in the lists.
            auto_start (bool | Unset): Start this profile when Harness Studio launches.
            updated_at (int | Unset):
    """

    id: str
    name: str
    config: LaunchConfig
    model_id: None | str | Unset = UNSET
    description: str | Unset = UNSET
    auto_start: bool | Unset = UNSET
    updated_at: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        name = self.name

        config = self.config.to_dict()

        model_id: None | str | Unset
        if isinstance(self.model_id, Unset):
            model_id = UNSET
        else:
            model_id = self.model_id

        description = self.description

        auto_start = self.auto_start

        updated_at = self.updated_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "config": config,
            }
        )
        if model_id is not UNSET:
            field_dict["model_id"] = model_id
        if description is not UNSET:
            field_dict["description"] = description
        if auto_start is not UNSET:
            field_dict["auto_start"] = auto_start
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.launch_config import LaunchConfig  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        config = LaunchConfig.from_dict(d.pop("config"))

        def _parse_model_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model_id = _parse_model_id(d.pop("model_id", UNSET))

        description = d.pop("description", UNSET)

        auto_start = d.pop("auto_start", UNSET)

        updated_at = d.pop("updated_at", UNSET)

        profile_view = cls(
            id=id,
            name=name,
            config=config,
            model_id=model_id,
            description=description,
            auto_start=auto_start,
            updated_at=updated_at,
        )

        profile_view.additional_properties = d
        return profile_view

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
