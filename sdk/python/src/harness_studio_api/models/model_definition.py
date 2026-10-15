from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.adapter_ref import AdapterRef
    from ..models.b_tree_map import BTreeMap
    from ..models.draft_spec import DraftSpec
    from ..models.model_ref_type_0 import ModelRefType0
    from ..models.model_ref_type_1 import ModelRefType1


T = TypeVar("T", bound="ModelDefinition")


@_attrs_define
class ModelDefinition:
    """A logical model: what clients ask for by `alias`.

    Attributes:
        id (str):
        alias (str): API `model` name; unique (case-insensitive), `[A-Za-z0-9._:-]`, see [`validate_alias`].
        base (ModelRefType0 | ModelRefType1): Base weights of a definition.
        description (str | Unset):
        mmproj (None | str | Unset):
        adapters (list[AdapterRef] | Unset):
        draft (DraftSpec | None | Unset):
        chat_template (None | str | Unset): Built-in template name or template file (`--chat-template`).
        chat_template_kwargs (None | str | Unset): JSON object text for `--chat-template-kwargs`.
        extra_args (None | str | Unset): Extra command-line arguments (same rules as the Dashboard's custom args field).
        load (BTreeMap | Unset):
        default_preset_id (None | str | Unset):
        ttl_seconds (int | None | Unset): Reserved for the gateway (idle unload after N seconds).
        pinned (bool | Unset): Reserved for the gateway (never evicted).
        capture_content (bool | Unset): Store the request and response content of gateway requests to this definition in
            `analytics.db`. Off by default: only metadata is recorded.
        content_retention_days (int | None | Unset): Days captured content is kept (default
            [`crate::analytics::DEFAULT_CONTENT_RETENTION_DAYS`]).
        auto_created (bool | Unset): Created by Harness Studio itself (v2 migration, a profile saved without a
            definition,
            the Dashboard session). Deleted again when the profile that owned it goes away and
            nothing else references it. Cleared as soon as the user edits the definition.
        created_at (int | Unset):
        updated_at (int | Unset):
    """

    id: str
    alias: str
    base: ModelRefType0 | ModelRefType1
    description: str | Unset = UNSET
    mmproj: None | str | Unset = UNSET
    adapters: list[AdapterRef] | Unset = UNSET
    draft: DraftSpec | None | Unset = UNSET
    chat_template: None | str | Unset = UNSET
    chat_template_kwargs: None | str | Unset = UNSET
    extra_args: None | str | Unset = UNSET
    load: BTreeMap | Unset = UNSET
    default_preset_id: None | str | Unset = UNSET
    ttl_seconds: int | None | Unset = UNSET
    pinned: bool | Unset = UNSET
    capture_content: bool | Unset = UNSET
    content_retention_days: int | None | Unset = UNSET
    auto_created: bool | Unset = UNSET
    created_at: int | Unset = UNSET
    updated_at: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.draft_spec import DraftSpec  # noqa: PLC0415
        from ..models.model_ref_type_0 import ModelRefType0  # noqa: PLC0415

        id = self.id

        alias = self.alias

        base: dict[str, Any]
        if isinstance(self.base, ModelRefType0):
            base = self.base.to_dict()
        else:
            base = self.base.to_dict()

        description = self.description

        mmproj: None | str | Unset
        if isinstance(self.mmproj, Unset):
            mmproj = UNSET
        else:
            mmproj = self.mmproj

        adapters: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.adapters, Unset):
            adapters = []
            for adapters_item_data in self.adapters:
                adapters_item = adapters_item_data.to_dict()
                adapters.append(adapters_item)

        draft: dict[str, Any] | None | Unset
        if isinstance(self.draft, Unset):
            draft = UNSET
        elif isinstance(self.draft, DraftSpec):
            draft = self.draft.to_dict()
        else:
            draft = self.draft

        chat_template: None | str | Unset
        if isinstance(self.chat_template, Unset):
            chat_template = UNSET
        else:
            chat_template = self.chat_template

        chat_template_kwargs: None | str | Unset
        if isinstance(self.chat_template_kwargs, Unset):
            chat_template_kwargs = UNSET
        else:
            chat_template_kwargs = self.chat_template_kwargs

        extra_args: None | str | Unset
        if isinstance(self.extra_args, Unset):
            extra_args = UNSET
        else:
            extra_args = self.extra_args

        load: dict[str, Any] | Unset = UNSET
        if not isinstance(self.load, Unset):
            load = self.load.to_dict()

        default_preset_id: None | str | Unset
        if isinstance(self.default_preset_id, Unset):
            default_preset_id = UNSET
        else:
            default_preset_id = self.default_preset_id

        ttl_seconds: int | None | Unset
        if isinstance(self.ttl_seconds, Unset):
            ttl_seconds = UNSET
        else:
            ttl_seconds = self.ttl_seconds

        pinned = self.pinned

        capture_content = self.capture_content

        content_retention_days: int | None | Unset
        if isinstance(self.content_retention_days, Unset):
            content_retention_days = UNSET
        else:
            content_retention_days = self.content_retention_days

        auto_created = self.auto_created

        created_at = self.created_at

        updated_at = self.updated_at

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "alias": alias,
                "base": base,
            }
        )
        if description is not UNSET:
            field_dict["description"] = description
        if mmproj is not UNSET:
            field_dict["mmproj"] = mmproj
        if adapters is not UNSET:
            field_dict["adapters"] = adapters
        if draft is not UNSET:
            field_dict["draft"] = draft
        if chat_template is not UNSET:
            field_dict["chat_template"] = chat_template
        if chat_template_kwargs is not UNSET:
            field_dict["chat_template_kwargs"] = chat_template_kwargs
        if extra_args is not UNSET:
            field_dict["extra_args"] = extra_args
        if load is not UNSET:
            field_dict["load"] = load
        if default_preset_id is not UNSET:
            field_dict["default_preset_id"] = default_preset_id
        if ttl_seconds is not UNSET:
            field_dict["ttl_seconds"] = ttl_seconds
        if pinned is not UNSET:
            field_dict["pinned"] = pinned
        if capture_content is not UNSET:
            field_dict["capture_content"] = capture_content
        if content_retention_days is not UNSET:
            field_dict["content_retention_days"] = content_retention_days
        if auto_created is not UNSET:
            field_dict["auto_created"] = auto_created
        if created_at is not UNSET:
            field_dict["created_at"] = created_at
        if updated_at is not UNSET:
            field_dict["updated_at"] = updated_at

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.adapter_ref import AdapterRef  # noqa: PLC0415
        from ..models.b_tree_map import BTreeMap  # noqa: PLC0415
        from ..models.draft_spec import DraftSpec  # noqa: PLC0415
        from ..models.model_ref_type_0 import ModelRefType0  # noqa: PLC0415
        from ..models.model_ref_type_1 import ModelRefType1  # noqa: PLC0415

        d = dict(src_dict)
        id = d.pop("id")

        alias = d.pop("alias")

        def _parse_base(data: object) -> ModelRefType0 | ModelRefType1:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_model_ref_type_0 = ModelRefType0.from_dict(data)

                return componentsschemas_model_ref_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_model_ref_type_1 = ModelRefType1.from_dict(data)

            return componentsschemas_model_ref_type_1

        base = _parse_base(d.pop("base"))

        description = d.pop("description", UNSET)

        def _parse_mmproj(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mmproj = _parse_mmproj(d.pop("mmproj", UNSET))

        _adapters = d.pop("adapters", UNSET)
        adapters: list[AdapterRef] | Unset = UNSET
        if _adapters is not UNSET:
            adapters = []
            for adapters_item_data in _adapters:
                adapters_item = AdapterRef.from_dict(adapters_item_data)

                adapters.append(adapters_item)

        def _parse_draft(data: object) -> DraftSpec | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                draft_type_1 = DraftSpec.from_dict(data)

                return draft_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(DraftSpec | None | Unset, data)

        draft = _parse_draft(d.pop("draft", UNSET))

        def _parse_chat_template(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        chat_template = _parse_chat_template(d.pop("chat_template", UNSET))

        def _parse_chat_template_kwargs(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        chat_template_kwargs = _parse_chat_template_kwargs(d.pop("chat_template_kwargs", UNSET))

        def _parse_extra_args(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        extra_args = _parse_extra_args(d.pop("extra_args", UNSET))

        _load = d.pop("load", UNSET)
        load: BTreeMap | Unset
        if isinstance(_load, Unset):
            load = UNSET
        else:
            load = BTreeMap.from_dict(_load)

        def _parse_default_preset_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        default_preset_id = _parse_default_preset_id(d.pop("default_preset_id", UNSET))

        def _parse_ttl_seconds(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ttl_seconds = _parse_ttl_seconds(d.pop("ttl_seconds", UNSET))

        pinned = d.pop("pinned", UNSET)

        capture_content = d.pop("capture_content", UNSET)

        def _parse_content_retention_days(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        content_retention_days = _parse_content_retention_days(
            d.pop("content_retention_days", UNSET)
        )

        auto_created = d.pop("auto_created", UNSET)

        created_at = d.pop("created_at", UNSET)

        updated_at = d.pop("updated_at", UNSET)

        model_definition = cls(
            id=id,
            alias=alias,
            base=base,
            description=description,
            mmproj=mmproj,
            adapters=adapters,
            draft=draft,
            chat_template=chat_template,
            chat_template_kwargs=chat_template_kwargs,
            extra_args=extra_args,
            load=load,
            default_preset_id=default_preset_id,
            ttl_seconds=ttl_seconds,
            pinned=pinned,
            capture_content=capture_content,
            content_retention_days=content_retention_days,
            auto_created=auto_created,
            created_at=created_at,
            updated_at=updated_at,
        )

        model_definition.additional_properties = d
        return model_definition

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
