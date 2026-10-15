from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

if TYPE_CHECKING:
    from ..models.server_ram import ServerRam


T = TypeVar("T", bound="MemoryBreakdown")


@_attrs_define
class MemoryBreakdown:
    """Used memory (system RAM or VRAM) split into Harness Studio, its llama-server processes
    and everything else. Process figures are working sets (RAM) or dedicated GPU memory
    (VRAM); shared RAM pages can be counted in more than one process, so `other_bytes`
    saturates at 0 instead of going negative.

        Attributes:
            harness_bytes (int): This app's process and its child processes (the WebView2 ones on Windows).
            servers (list[ServerRam]):
            other_bytes (int): Used - Harness Studio - servers.
    """

    harness_bytes: int
    servers: list[ServerRam]
    other_bytes: int
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        harness_bytes = self.harness_bytes

        servers = []
        for servers_item_data in self.servers:
            servers_item = servers_item_data.to_dict()
            servers.append(servers_item)

        other_bytes = self.other_bytes

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "harness_bytes": harness_bytes,
                "servers": servers,
                "other_bytes": other_bytes,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.server_ram import ServerRam  # noqa: PLC0415

        d = dict(src_dict)
        harness_bytes = d.pop("harness_bytes")

        servers = []
        _servers = d.pop("servers")
        for servers_item_data in _servers:
            servers_item = ServerRam.from_dict(servers_item_data)

            servers.append(servers_item)

        other_bytes = d.pop("other_bytes")

        memory_breakdown = cls(
            harness_bytes=harness_bytes,
            servers=servers,
            other_bytes=other_bytes,
        )

        memory_breakdown.additional_properties = d
        return memory_breakdown

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
