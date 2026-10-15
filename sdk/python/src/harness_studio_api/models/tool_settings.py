from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.tool_policy import ToolPolicy, check_tool_policy
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.tool_settings_mcp_classes_type_0 import ToolSettingsMcpClassesType0
    from ..models.web_search_settings import WebSearchSettings


T = TypeVar("T", bound="ToolSettings")


@_attrs_define
class ToolSettings:
    """`settings.tools`.

    Attributes:
        policy (ToolPolicy | Unset): `settings.tools.policy`.
        trusted (list[str] | Unset): Tools the user trusts (names), honoured under "allow trusted".
        workdir (str | Unset): Working directory of file tools and the shell when the caller names none. Empty = none:
            such calls fail with `no_workspace` instead of guessing. Default: ''.
        workspaces (list[str] | None | Unset): Saved folders in most-recently-used order. None migrates legacy settings
            and
            preserves the list when an older client saves only the original fields.
        mcp_classes (None | ToolSettingsMcpClassesType0 | Unset): The user's permission class for an MCP tool (by its
            registry name, `<server>_<tool>`),
            replacing the class its annotations give it. Only MCP tools: a built-in tool's class is
            fixed in the code.

            `None` = not given: saving settings from a screen that does not know the table keeps
            the stored one (`save_settings`); `Some(empty)` clears it.
        web_search (WebSearchSettings | Unset):
        mcp_expose (bool | Unset): Offer the built-in tools to MCP clients (`harness mcp`, `/mcp`) as well. Off by
            default
            so existing clients do not suddenly see file and shell tools. The policy above still
            decides each call; MCP cannot ask, so only calls the policy allows run. Default: False.
    """

    policy: ToolPolicy | Unset = UNSET
    trusted: list[str] | Unset = UNSET
    workdir: str | Unset = ""
    workspaces: list[str] | None | Unset = UNSET
    mcp_classes: None | ToolSettingsMcpClassesType0 | Unset = UNSET
    web_search: WebSearchSettings | Unset = UNSET
    mcp_expose: bool | Unset = False
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.tool_settings_mcp_classes_type_0 import (
            ToolSettingsMcpClassesType0,  # noqa: PLC0415
        )

        policy: str | Unset = UNSET
        if not isinstance(self.policy, Unset):
            policy = self.policy

        trusted: list[str] | Unset = UNSET
        if not isinstance(self.trusted, Unset):
            trusted = self.trusted

        workdir = self.workdir

        workspaces: list[str] | None | Unset
        if isinstance(self.workspaces, Unset):
            workspaces = UNSET
        elif isinstance(self.workspaces, list):
            workspaces = self.workspaces

        else:
            workspaces = self.workspaces

        mcp_classes: dict[str, Any] | None | Unset
        if isinstance(self.mcp_classes, Unset):
            mcp_classes = UNSET
        elif isinstance(self.mcp_classes, ToolSettingsMcpClassesType0):
            mcp_classes = self.mcp_classes.to_dict()
        else:
            mcp_classes = self.mcp_classes

        web_search: dict[str, Any] | Unset = UNSET
        if not isinstance(self.web_search, Unset):
            web_search = self.web_search.to_dict()

        mcp_expose = self.mcp_expose

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if policy is not UNSET:
            field_dict["policy"] = policy
        if trusted is not UNSET:
            field_dict["trusted"] = trusted
        if workdir is not UNSET:
            field_dict["workdir"] = workdir
        if workspaces is not UNSET:
            field_dict["workspaces"] = workspaces
        if mcp_classes is not UNSET:
            field_dict["mcp_classes"] = mcp_classes
        if web_search is not UNSET:
            field_dict["web_search"] = web_search
        if mcp_expose is not UNSET:
            field_dict["mcp_expose"] = mcp_expose

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tool_settings_mcp_classes_type_0 import (
            ToolSettingsMcpClassesType0,  # noqa: PLC0415
        )
        from ..models.web_search_settings import WebSearchSettings  # noqa: PLC0415

        d = dict(src_dict)
        _policy = d.pop("policy", UNSET)
        policy: ToolPolicy | Unset
        if isinstance(_policy, Unset):
            policy = UNSET
        else:
            policy = check_tool_policy(_policy)

        trusted = cast(list[str], d.pop("trusted", UNSET))

        workdir = d.pop("workdir", UNSET)

        def _parse_workspaces(data: object) -> list[str] | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, list):
                    raise TypeError()
                workspaces_type_0 = cast(list[str], data)

                return workspaces_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(list[str] | None | Unset, data)

        workspaces = _parse_workspaces(d.pop("workspaces", UNSET))

        def _parse_mcp_classes(data: object) -> None | ToolSettingsMcpClassesType0 | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                mcp_classes_type_0 = ToolSettingsMcpClassesType0.from_dict(data)

                return mcp_classes_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | ToolSettingsMcpClassesType0 | Unset, data)

        mcp_classes = _parse_mcp_classes(d.pop("mcp_classes", UNSET))

        _web_search = d.pop("web_search", UNSET)
        web_search: WebSearchSettings | Unset
        if isinstance(_web_search, Unset):
            web_search = UNSET
        else:
            web_search = WebSearchSettings.from_dict(_web_search)

        mcp_expose = d.pop("mcp_expose", UNSET)

        tool_settings = cls(
            policy=policy,
            trusted=trusted,
            workdir=workdir,
            workspaces=workspaces,
            mcp_classes=mcp_classes,
            web_search=web_search,
            mcp_expose=mcp_expose,
        )

        tool_settings.additional_properties = d
        return tool_settings

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
