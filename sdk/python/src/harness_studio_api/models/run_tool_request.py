from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.run_tool_request_args import RunToolRequestArgs


T = TypeVar("T", bound="RunToolRequest")


@_attrs_define
class RunToolRequest:
    """
    Attributes:
        args (RunToolRequestArgs | Unset): The tool's arguments (its `input_schema`).
        workdir (None | str | Unset): Working directory of file tools and the shell; default: `workdir` of the policy
            settings. File tools cannot leave it.
        approved (bool | Unset): The caller asked the user and got a yes (see the module docs).
        client (None | str | Unset): Client label for the tool log (default: the token's name, or `api`).
        conversation (None | str | Unset): For the tool log.
        model (None | str | Unset): For the tool log: the model that wrote the call.
        timeout_secs (int | None | Unset): Time limit in seconds (default 120, at most 600).
    """

    args: RunToolRequestArgs | Unset = UNSET
    workdir: None | str | Unset = UNSET
    approved: bool | Unset = UNSET
    client: None | str | Unset = UNSET
    conversation: None | str | Unset = UNSET
    model: None | str | Unset = UNSET
    timeout_secs: int | None | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        args: dict[str, Any] | Unset = UNSET
        if not isinstance(self.args, Unset):
            args = self.args.to_dict()

        workdir: None | str | Unset
        if isinstance(self.workdir, Unset):
            workdir = UNSET
        else:
            workdir = self.workdir

        approved = self.approved

        client: None | str | Unset
        if isinstance(self.client, Unset):
            client = UNSET
        else:
            client = self.client

        conversation: None | str | Unset
        if isinstance(self.conversation, Unset):
            conversation = UNSET
        else:
            conversation = self.conversation

        model: None | str | Unset
        if isinstance(self.model, Unset):
            model = UNSET
        else:
            model = self.model

        timeout_secs: int | None | Unset
        if isinstance(self.timeout_secs, Unset):
            timeout_secs = UNSET
        else:
            timeout_secs = self.timeout_secs

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update({})
        if args is not UNSET:
            field_dict["args"] = args
        if workdir is not UNSET:
            field_dict["workdir"] = workdir
        if approved is not UNSET:
            field_dict["approved"] = approved
        if client is not UNSET:
            field_dict["client"] = client
        if conversation is not UNSET:
            field_dict["conversation"] = conversation
        if model is not UNSET:
            field_dict["model"] = model
        if timeout_secs is not UNSET:
            field_dict["timeout_secs"] = timeout_secs

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.run_tool_request_args import RunToolRequestArgs  # noqa: PLC0415

        d = dict(src_dict)
        _args = d.pop("args", UNSET)
        args: RunToolRequestArgs | Unset
        if isinstance(_args, Unset):
            args = UNSET
        else:
            args = RunToolRequestArgs.from_dict(_args)

        def _parse_workdir(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        workdir = _parse_workdir(d.pop("workdir", UNSET))

        approved = d.pop("approved", UNSET)

        def _parse_client(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        client = _parse_client(d.pop("client", UNSET))

        def _parse_conversation(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        conversation = _parse_conversation(d.pop("conversation", UNSET))

        def _parse_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model = _parse_model(d.pop("model", UNSET))

        def _parse_timeout_secs(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        timeout_secs = _parse_timeout_secs(d.pop("timeout_secs", UNSET))

        run_tool_request = cls(
            args=args,
            workdir=workdir,
            approved=approved,
            client=client,
            conversation=conversation,
            model=model,
            timeout_secs=timeout_secs,
        )

        run_tool_request.additional_properties = d
        return run_tool_request

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
