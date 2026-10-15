from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.router_model_state import RouterModelState, check_router_model_state
from ..types import UNSET, Unset

T = TypeVar("T", bound="RouterModel")


@_attrs_define
class RouterModel:
    """One model a router serves.

    Attributes:
        id (str): The name clients send as `model`.
        state (RouterModelState): Load state of one model of a router.
        source (None | str | Unset): Where the router found it: `models_dir`, `preset` or `cache`.
        path (None | str | Unset): The model file, when the router's arguments name one.
        context_length (int | None | Unset):
        exit_code (int | None | Unset): Exit code of the last failed load.
    """

    id: str
    state: RouterModelState
    source: None | str | Unset = UNSET
    path: None | str | Unset = UNSET
    context_length: int | None | Unset = UNSET
    exit_code: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        state: str = self.state

        source: None | str | Unset
        if isinstance(self.source, Unset):
            source = UNSET
        else:
            source = self.source

        path: None | str | Unset
        if isinstance(self.path, Unset):
            path = UNSET
        else:
            path = self.path

        context_length: int | None | Unset
        if isinstance(self.context_length, Unset):
            context_length = UNSET
        else:
            context_length = self.context_length

        exit_code: int | None | Unset
        if isinstance(self.exit_code, Unset):
            exit_code = UNSET
        else:
            exit_code = self.exit_code

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "state": state,
            }
        )
        if source is not UNSET:
            field_dict["source"] = source
        if path is not UNSET:
            field_dict["path"] = path
        if context_length is not UNSET:
            field_dict["context_length"] = context_length
        if exit_code is not UNSET:
            field_dict["exit_code"] = exit_code

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        state = check_router_model_state(d.pop("state"))

        def _parse_source(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source = _parse_source(d.pop("source", UNSET))

        def _parse_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        path = _parse_path(d.pop("path", UNSET))

        def _parse_context_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        context_length = _parse_context_length(d.pop("context_length", UNSET))

        def _parse_exit_code(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        exit_code = _parse_exit_code(d.pop("exit_code", UNSET))

        router_model = cls(
            id=id,
            state=state,
            source=source,
            path=path,
            context_length=context_length,
            exit_code=exit_code,
        )

        router_model.additional_properties = d
        return router_model

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
