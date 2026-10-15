from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.tool_error_code import ToolErrorCode, check_tool_error_code
from ..types import UNSET, Unset

T = TypeVar("T", bound="RunToolResponse")


@_attrs_define
class RunToolResponse:
    """
    Attributes:
        content (str): The text the model reads (the error sentence when `error` is set).
        duration_ms (int):
        truncated (bool): `content` was cut at the result size limit.
        id (str): Id of the call in the tool log.
        tool (str):
        error (None | ToolErrorCode | Unset):
        network_target (None | str | Unset):
    """

    content: str
    duration_ms: int
    truncated: bool
    id: str
    tool: str
    error: None | ToolErrorCode | Unset = UNSET
    network_target: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        content = self.content

        duration_ms = self.duration_ms

        truncated = self.truncated

        id = self.id

        tool = self.tool

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        elif isinstance(self.error, str):
            error = self.error
        else:
            error = self.error

        network_target: None | str | Unset
        if isinstance(self.network_target, Unset):
            network_target = UNSET
        else:
            network_target = self.network_target

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "content": content,
                "duration_ms": duration_ms,
                "truncated": truncated,
                "id": id,
                "tool": tool,
            }
        )
        if error is not UNSET:
            field_dict["error"] = error
        if network_target is not UNSET:
            field_dict["network_target"] = network_target

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        content = d.pop("content")

        duration_ms = d.pop("duration_ms")

        truncated = d.pop("truncated")

        id = d.pop("id")

        tool = d.pop("tool")

        def _parse_error(data: object) -> None | ToolErrorCode | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                error_type_1 = check_tool_error_code(data)

                return error_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ToolErrorCode | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_network_target(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        network_target = _parse_network_target(d.pop("network_target", UNSET))

        run_tool_response = cls(
            content=content,
            duration_ms=duration_ms,
            truncated=truncated,
            id=id,
            tool=tool,
            error=error,
            network_target=network_target,
        )

        run_tool_response.additional_properties = d
        return run_tool_response

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
