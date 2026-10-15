from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

T = TypeVar("T", bound="RuntimeFlag")


@_attrs_define
class RuntimeFlag:
    """A `--help` flag for the "all flags" explorer.

    Attributes:
        canonical (str): First long (`--x`) spelling that is not a `--no-` negation; else the first spelling.
        aliases (list[str]): Every spelling on the header line (including `canonical` and negations), in order.
        description (str): Description with continuation lines joined by single spaces.
        protected (bool): Harness Studio manages this flag itself; it can't be added as a custom argument.
        negations (list[str] | Unset): Negative spellings (`--no-x`, `-no-x`).
        value_hint (None | str | Unset): Value placeholder (`N`, `FNAME`, `[on|off|auto]`), `None` for switches.
        value_optional (bool | Unset): The value is optional (`[on|off|auto]`).
        enum_values (list[str] | Unset): Allowed values when the help lists them (hint, "allowed values:" or a `- x:`
            list).
        env (None | str | Unset):
        default (None | str | Unset):
        section (None | str | Unset): `common`, `sampling`, `speculative`, `example-specific`, ...
        deprecated (bool | Unset): Marked `(DEPRECATED)` in the help.
        removed (bool | Unset): Still listed but "the argument has been removed"; never emitted.
        param_id (None | str | Unset): Catalog parameter that already covers this flag (set it there instead).
    """

    canonical: str
    aliases: list[str]
    description: str
    protected: bool
    negations: list[str] | Unset = UNSET
    value_hint: None | str | Unset = UNSET
    value_optional: bool | Unset = UNSET
    enum_values: list[str] | Unset = UNSET
    env: None | str | Unset = UNSET
    default: None | str | Unset = UNSET
    section: None | str | Unset = UNSET
    deprecated: bool | Unset = UNSET
    removed: bool | Unset = UNSET
    param_id: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        canonical = self.canonical

        aliases = self.aliases

        description = self.description

        protected = self.protected

        negations: list[str] | Unset = UNSET
        if not isinstance(self.negations, Unset):
            negations = self.negations

        value_hint: None | str | Unset
        if isinstance(self.value_hint, Unset):
            value_hint = UNSET
        else:
            value_hint = self.value_hint

        value_optional = self.value_optional

        enum_values: list[str] | Unset = UNSET
        if not isinstance(self.enum_values, Unset):
            enum_values = self.enum_values

        env: None | str | Unset
        if isinstance(self.env, Unset):
            env = UNSET
        else:
            env = self.env

        default: None | str | Unset
        if isinstance(self.default, Unset):
            default = UNSET
        else:
            default = self.default

        section: None | str | Unset
        if isinstance(self.section, Unset):
            section = UNSET
        else:
            section = self.section

        deprecated = self.deprecated

        removed = self.removed

        param_id: None | str | Unset
        if isinstance(self.param_id, Unset):
            param_id = UNSET
        else:
            param_id = self.param_id

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "canonical": canonical,
                "aliases": aliases,
                "description": description,
                "protected": protected,
            }
        )
        if negations is not UNSET:
            field_dict["negations"] = negations
        if value_hint is not UNSET:
            field_dict["value_hint"] = value_hint
        if value_optional is not UNSET:
            field_dict["value_optional"] = value_optional
        if enum_values is not UNSET:
            field_dict["enum_values"] = enum_values
        if env is not UNSET:
            field_dict["env"] = env
        if default is not UNSET:
            field_dict["default"] = default
        if section is not UNSET:
            field_dict["section"] = section
        if deprecated is not UNSET:
            field_dict["deprecated"] = deprecated
        if removed is not UNSET:
            field_dict["removed"] = removed
        if param_id is not UNSET:
            field_dict["param_id"] = param_id

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        canonical = d.pop("canonical")

        aliases = cast(list[str], d.pop("aliases"))

        description = d.pop("description")

        protected = d.pop("protected")

        negations = cast(list[str], d.pop("negations", UNSET))

        def _parse_value_hint(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        value_hint = _parse_value_hint(d.pop("value_hint", UNSET))

        value_optional = d.pop("value_optional", UNSET)

        enum_values = cast(list[str], d.pop("enum_values", UNSET))

        def _parse_env(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        env = _parse_env(d.pop("env", UNSET))

        def _parse_default(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        default = _parse_default(d.pop("default", UNSET))

        def _parse_section(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        section = _parse_section(d.pop("section", UNSET))

        deprecated = d.pop("deprecated", UNSET)

        removed = d.pop("removed", UNSET)

        def _parse_param_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        param_id = _parse_param_id(d.pop("param_id", UNSET))

        runtime_flag = cls(
            canonical=canonical,
            aliases=aliases,
            description=description,
            protected=protected,
            negations=negations,
            value_hint=value_hint,
            value_optional=value_optional,
            enum_values=enum_values,
            env=env,
            default=default,
            section=section,
            deprecated=deprecated,
            removed=removed,
            param_id=param_id,
        )

        runtime_flag.additional_properties = d
        return runtime_flag

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
