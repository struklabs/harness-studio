from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="ServerTools")


@_attrs_define
class ServerTools:
    """What the gateway fulfils itself for one model definition (`GatewaySettings::server_tools`).

    A request for such a model is completed locally with the core streaming agent loop
    (`harness_core::tools::agent_stream`): the model gets the tools, the gateway runs the calls
    through the tool engine and the client only sees the final answer. Only tools the user's
    tool policy lets run **without asking** may run (the gateway has nobody to ask); anything
    else is a clear error, never a silent skip.

        Attributes:
            web_search (bool | Unset): Fulfil the hosted `web_search` tool a client asks for (Responses `web_search`, Chat
                `web_search_options`, Anthropic `web_search_*`) with Harness's `web_search` tool and the
                search provider the user configured (Settings > Tools). Default: False.
            tools (list[str] | Unset): Harness tools (registry names; MCP tools are `<server>_<tool>`) offered to the model
                on
                every Chat / Responses / Messages request and run by the gateway. A tool the client
                sends with the same name stays the client's.
    """

    web_search: bool | Unset = False
    tools: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        web_search = self.web_search

        tools: list[str] | Unset = UNSET
        if not isinstance(self.tools, Unset):
            tools = self.tools

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if web_search is not UNSET:
            field_dict["web_search"] = web_search
        if tools is not UNSET:
            field_dict["tools"] = tools

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        web_search = d.pop("web_search", UNSET)

        tools = cast(list[str], d.pop("tools", UNSET))

        server_tools = cls(
            web_search=web_search,
            tools=tools,
        )

        server_tools.additional_properties = d
        return server_tools

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
