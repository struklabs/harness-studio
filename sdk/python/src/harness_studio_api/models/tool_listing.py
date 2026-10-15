from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.tool_policy import ToolPolicy, check_tool_policy
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.tool_entry import ToolEntry
    from ..models.tool_limits import ToolLimits
    from ..models.web_search_settings import WebSearchSettings


T = TypeVar("T", bound="ToolListing")


@_attrs_define
class ToolListing:
    """`GET /tools`: the tools and the policy they are judged by.

    Attributes:
        policy (ToolPolicy): `settings.tools.policy`.
        trusted (list[str]):
        workdir (str): Default working directory (empty = none).
        web_search (WebSearchSettings):
        limits (ToolLimits): The limits as clients see them (`GET /tools`, the Playground's `tools_list`).
        tools (list[ToolEntry]):
        workspaces (list[str] | Unset):
        missing_workspaces (list[str] | Unset):
        mcp_expose (bool | Unset): `settings.tools.mcp_expose`: the built-in tools are offered to MCP clients.
    """

    policy: ToolPolicy
    trusted: list[str]
    workdir: str
    web_search: WebSearchSettings
    limits: ToolLimits
    tools: list[ToolEntry]
    workspaces: list[str] | Unset = UNSET
    missing_workspaces: list[str] | Unset = UNSET
    mcp_expose: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        policy: str = self.policy

        trusted = self.trusted

        workdir = self.workdir

        web_search = self.web_search.to_dict()

        limits = self.limits.to_dict()

        tools = []
        for tools_item_data in self.tools:
            tools_item = tools_item_data.to_dict()
            tools.append(tools_item)

        workspaces: list[str] | Unset = UNSET
        if not isinstance(self.workspaces, Unset):
            workspaces = self.workspaces

        missing_workspaces: list[str] | Unset = UNSET
        if not isinstance(self.missing_workspaces, Unset):
            missing_workspaces = self.missing_workspaces

        mcp_expose = self.mcp_expose

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "policy": policy,
                "trusted": trusted,
                "workdir": workdir,
                "web_search": web_search,
                "limits": limits,
                "tools": tools,
            }
        )
        if workspaces is not UNSET:
            field_dict["workspaces"] = workspaces
        if missing_workspaces is not UNSET:
            field_dict["missing_workspaces"] = missing_workspaces
        if mcp_expose is not UNSET:
            field_dict["mcp_expose"] = mcp_expose

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.tool_entry import ToolEntry  # noqa: PLC0415
        from ..models.tool_limits import ToolLimits  # noqa: PLC0415
        from ..models.web_search_settings import WebSearchSettings  # noqa: PLC0415

        d = dict(src_dict)
        policy = check_tool_policy(d.pop("policy"))

        trusted = cast(list[str], d.pop("trusted"))

        workdir = d.pop("workdir")

        web_search = WebSearchSettings.from_dict(d.pop("web_search"))

        limits = ToolLimits.from_dict(d.pop("limits"))

        tools = []
        _tools = d.pop("tools")
        for tools_item_data in _tools:
            tools_item = ToolEntry.from_dict(tools_item_data)

            tools.append(tools_item)

        workspaces = cast(list[str], d.pop("workspaces", UNSET))

        missing_workspaces = cast(list[str], d.pop("missing_workspaces", UNSET))

        mcp_expose = d.pop("mcp_expose", UNSET)

        tool_listing = cls(
            policy=policy,
            trusted=trusted,
            workdir=workdir,
            web_search=web_search,
            limits=limits,
            tools=tools,
            workspaces=workspaces,
            missing_workspaces=missing_workspaces,
            mcp_expose=mcp_expose,
        )

        tool_listing.additional_properties = d
        return tool_listing

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
