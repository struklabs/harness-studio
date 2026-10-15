from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.keep_alive_type_0 import KeepAliveType0, check_keep_alive_type_0
from ..models.slot_state import SlotState, check_slot_state
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.keep_alive_type_1 import KeepAliveType1
    from ..models.load_adaptation import LoadAdaptation


T = TypeVar("T", bound="ModelSlot")


@_attrs_define
class ModelSlot:
    """One definition as the gateway sees it (GUI table, Control API `/gateway`).

    Attributes:
        definition_id (str):
        alias (str):
        state (SlotState): Where a definition stands in the scheduler.
        managed (bool): Loaded by the gateway (counts against the limits, may be evicted / unloaded by TTL).
            `false` = a server the user started; the gateway only routes to it.
        pinned (bool):
        in_flight (int):
        queued (int):
        concurrency (int): Requests that may run at once (llama-server slots).
        server_id (None | str | Unset):
        port (int | None | Unset):
        ttl_secs (int | None | Unset): Effective idle TTL (Ollama `keep_alive` override, else the definition's, else the
            gateway default).
        keep_alive (KeepAliveType0 | KeepAliveType1 | None | Unset):
        waited (int | Unset): Requests that had to wait for a free slot since the model was loaded (all slots were
            busy): a hint to give the model more concurrent requests at its next load.
        token_pool (int | Unset): Token budget of the shared KV pool (`-c` with `--kv-unified` and several slots; 0 = no
            budget) and the part the running requests hold (their estimates).
        tokens_in_use (int | Unset):
        waited_tokens (int | Unset): Requests that had a free slot but waited for room in the shared KV pool since the
            model
            was loaded (more slots do not help these; a larger context does).
        idle_secs (int | None | Unset): Seconds since the last request finished.
        adapted (LoadAdaptation | None | Unset):
    """

    definition_id: str
    alias: str
    state: SlotState
    managed: bool
    pinned: bool
    in_flight: int
    queued: int
    concurrency: int
    server_id: None | str | Unset = UNSET
    port: int | None | Unset = UNSET
    ttl_secs: int | None | Unset = UNSET
    keep_alive: KeepAliveType0 | KeepAliveType1 | None | Unset = UNSET
    waited: int | Unset = UNSET
    token_pool: int | Unset = UNSET
    tokens_in_use: int | Unset = UNSET
    waited_tokens: int | Unset = UNSET
    idle_secs: int | None | Unset = UNSET
    adapted: LoadAdaptation | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.keep_alive_type_1 import KeepAliveType1  # noqa: PLC0415
        from ..models.load_adaptation import LoadAdaptation  # noqa: PLC0415

        definition_id = self.definition_id

        alias = self.alias

        state: str = self.state

        managed = self.managed

        pinned = self.pinned

        in_flight = self.in_flight

        queued = self.queued

        concurrency = self.concurrency

        server_id: None | str | Unset
        if isinstance(self.server_id, Unset):
            server_id = UNSET
        else:
            server_id = self.server_id

        port: int | None | Unset
        if isinstance(self.port, Unset):
            port = UNSET
        else:
            port = self.port

        ttl_secs: int | None | Unset
        if isinstance(self.ttl_secs, Unset):
            ttl_secs = UNSET
        else:
            ttl_secs = self.ttl_secs

        keep_alive: dict[str, Any] | None | str | Unset
        if isinstance(self.keep_alive, Unset):
            keep_alive = UNSET
        elif isinstance(self.keep_alive, str):
            keep_alive = self.keep_alive
        elif isinstance(self.keep_alive, KeepAliveType1):
            keep_alive = self.keep_alive.to_dict()
        else:
            keep_alive = self.keep_alive

        waited = self.waited

        token_pool = self.token_pool

        tokens_in_use = self.tokens_in_use

        waited_tokens = self.waited_tokens

        idle_secs: int | None | Unset
        if isinstance(self.idle_secs, Unset):
            idle_secs = UNSET
        else:
            idle_secs = self.idle_secs

        adapted: dict[str, Any] | None | Unset
        if isinstance(self.adapted, Unset):
            adapted = UNSET
        elif isinstance(self.adapted, LoadAdaptation):
            adapted = self.adapted.to_dict()
        else:
            adapted = self.adapted

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "definition_id": definition_id,
                "alias": alias,
                "state": state,
                "managed": managed,
                "pinned": pinned,
                "in_flight": in_flight,
                "queued": queued,
                "concurrency": concurrency,
            }
        )
        if server_id is not UNSET:
            field_dict["server_id"] = server_id
        if port is not UNSET:
            field_dict["port"] = port
        if ttl_secs is not UNSET:
            field_dict["ttl_secs"] = ttl_secs
        if keep_alive is not UNSET:
            field_dict["keep_alive"] = keep_alive
        if waited is not UNSET:
            field_dict["waited"] = waited
        if token_pool is not UNSET:
            field_dict["token_pool"] = token_pool
        if tokens_in_use is not UNSET:
            field_dict["tokens_in_use"] = tokens_in_use
        if waited_tokens is not UNSET:
            field_dict["waited_tokens"] = waited_tokens
        if idle_secs is not UNSET:
            field_dict["idle_secs"] = idle_secs
        if adapted is not UNSET:
            field_dict["adapted"] = adapted

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.keep_alive_type_1 import KeepAliveType1  # noqa: PLC0415
        from ..models.load_adaptation import LoadAdaptation  # noqa: PLC0415

        d = dict(src_dict)
        definition_id = d.pop("definition_id")

        alias = d.pop("alias")

        state = check_slot_state(d.pop("state"))

        managed = d.pop("managed")

        pinned = d.pop("pinned")

        in_flight = d.pop("in_flight")

        queued = d.pop("queued")

        concurrency = d.pop("concurrency")

        def _parse_server_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        server_id = _parse_server_id(d.pop("server_id", UNSET))

        def _parse_port(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        port = _parse_port(d.pop("port", UNSET))

        def _parse_ttl_secs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ttl_secs = _parse_ttl_secs(d.pop("ttl_secs", UNSET))

        def _parse_keep_alive(data: object) -> KeepAliveType0 | KeepAliveType1 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                componentsschemas_keep_alive_type_0 = check_keep_alive_type_0(data)

                return componentsschemas_keep_alive_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_keep_alive_type_1 = KeepAliveType1.from_dict(data)

                return componentsschemas_keep_alive_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(KeepAliveType0 | KeepAliveType1 | None | Unset, data)

        keep_alive = _parse_keep_alive(d.pop("keep_alive", UNSET))

        waited = d.pop("waited", UNSET)

        token_pool = d.pop("token_pool", UNSET)

        tokens_in_use = d.pop("tokens_in_use", UNSET)

        waited_tokens = d.pop("waited_tokens", UNSET)

        def _parse_idle_secs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        idle_secs = _parse_idle_secs(d.pop("idle_secs", UNSET))

        def _parse_adapted(data: object) -> LoadAdaptation | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                adapted_type_1 = LoadAdaptation.from_dict(data)

                return adapted_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LoadAdaptation | None | Unset, data)

        adapted = _parse_adapted(d.pop("adapted", UNSET))

        model_slot = cls(
            definition_id=definition_id,
            alias=alias,
            state=state,
            managed=managed,
            pinned=pinned,
            in_flight=in_flight,
            queued=queued,
            concurrency=concurrency,
            server_id=server_id,
            port=port,
            ttl_secs=ttl_secs,
            keep_alive=keep_alive,
            waited=waited,
            token_pool=token_pool,
            tokens_in_use=tokens_in_use,
            waited_tokens=waited_tokens,
            idle_secs=idle_secs,
            adapted=adapted,
        )

        model_slot.additional_properties = d
        return model_slot

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
