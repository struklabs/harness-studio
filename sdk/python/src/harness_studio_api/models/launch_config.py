from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.slot_origin import SlotOrigin, check_slot_origin
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.b_tree_map import BTreeMap


T = TypeVar("T", bound="LaunchConfig")


@_attrs_define
class LaunchConfig:
    """
    Attributes:
        model_path (str):
        gpu_layers (int):
        context_size (int):
        host (str):
        port (int): The listening port. With `port_auto` (or 0) it is only the last automatic pick.
        binary_path (str | Unset): Always overwritten by the backend (see `resolve_server_binary`).
        port_auto (bool | Unset): Harness picks the port (`ports::AUTO_RANGE`, 11440–11499): `port` is kept while it is
            free, else another one is taken silently. Off = the user chose `port`; a taken port is
            then an error naming the program that holds it. Port 0 means the same as on.
        threads (int | None | Unset):
        flash_attn (bool | Unset):
        cache_type_k (None | str | Unset):
        cache_type_v (None | str | Unset):
        parallel_slots (int | None | Unset):
        logical_batch (int | None | Unset):
        physical_batch (int | None | Unset):
        context_shift (bool | None | Unset):
        split_mode (None | str | Unset):
        main_gpu (int | None | Unset): Main GPU as an OS adapter index (`gpu::GpuAdapter::index`, what the GPU picker
            shows),
            not the runtime's device index; converted at launch (`devices::pair_adapters`).
        tensor_split (None | str | Unset):
        rope_scaling (None | str | Unset):
        rope_freq_base (float | None | Unset):
        rope_freq_scale (float | None | Unset):
        chat_template (None | str | Unset):
        api_key_ref (None | str | Unset): Keychain reference of the saved key (safe to store and to send to the UI).
        custom_args (None | str | Unset):
        draft_model_path (None | str | Unset):
        draft_gpu_layers (int | None | Unset):
        lora_path (None | str | Unset):
        lora_scale (float | None | Unset):
        mmproj_path (None | str | Unset):
        router_mode (bool | Unset):
        models_path (None | str | Unset):
        mcp_enabled (bool | Unset):
        mcp_config_path (None | str | Unset): Resolved by the backend at launch (never trusted from the frontend).
        params (BTreeMap | Unset):
        runtime (None | str | Unset): Runtime (llama-server build) of this launch: a `runtimes` registry id; `None` or
            `official` = Settings > Runtime. Resolved to `binary_path` by the orchestrator.
        alias (None | str | Unset): Served model id (`--alias`): the ModelDefinition's alias. `None` = the file stem.
        definition_id (None | str | Unset): The ModelDefinition this launch was resolved from (informational: lets the
            UI and the
            Dashboard session know which definition a server runs).
        launched_by (SlotOrigin | Unset): Who starts a server.
        auto_fields (list[str] | Unset): Settings (catalog ids) that hold the automatic choice, not a value anyone set:
            the
            context, layer count and slot count a form shows from the default-load rule
            (`full_offload`), and the layer count of a resolved definition launch. They are left
            out of every layer made from this config (`domain::launch::config_layer`), so a
            profile, definition or last session never freezes them as explicit values.
    """

    model_path: str
    gpu_layers: int
    context_size: int
    host: str
    port: int
    binary_path: str | Unset = UNSET
    port_auto: bool | Unset = UNSET
    threads: int | None | Unset = UNSET
    flash_attn: bool | Unset = UNSET
    cache_type_k: None | str | Unset = UNSET
    cache_type_v: None | str | Unset = UNSET
    parallel_slots: int | None | Unset = UNSET
    logical_batch: int | None | Unset = UNSET
    physical_batch: int | None | Unset = UNSET
    context_shift: bool | None | Unset = UNSET
    split_mode: None | str | Unset = UNSET
    main_gpu: int | None | Unset = UNSET
    tensor_split: None | str | Unset = UNSET
    rope_scaling: None | str | Unset = UNSET
    rope_freq_base: float | None | Unset = UNSET
    rope_freq_scale: float | None | Unset = UNSET
    chat_template: None | str | Unset = UNSET
    api_key_ref: None | str | Unset = UNSET
    custom_args: None | str | Unset = UNSET
    draft_model_path: None | str | Unset = UNSET
    draft_gpu_layers: int | None | Unset = UNSET
    lora_path: None | str | Unset = UNSET
    lora_scale: float | None | Unset = UNSET
    mmproj_path: None | str | Unset = UNSET
    router_mode: bool | Unset = UNSET
    models_path: None | str | Unset = UNSET
    mcp_enabled: bool | Unset = UNSET
    mcp_config_path: None | str | Unset = UNSET
    params: BTreeMap | Unset = UNSET
    runtime: None | str | Unset = UNSET
    alias: None | str | Unset = UNSET
    definition_id: None | str | Unset = UNSET
    launched_by: SlotOrigin | Unset = UNSET
    auto_fields: list[str] | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        model_path = self.model_path

        gpu_layers = self.gpu_layers

        context_size = self.context_size

        host = self.host

        port = self.port

        binary_path = self.binary_path

        port_auto = self.port_auto

        threads: int | None | Unset
        if isinstance(self.threads, Unset):
            threads = UNSET
        else:
            threads = self.threads

        flash_attn = self.flash_attn

        cache_type_k: None | str | Unset
        if isinstance(self.cache_type_k, Unset):
            cache_type_k = UNSET
        else:
            cache_type_k = self.cache_type_k

        cache_type_v: None | str | Unset
        if isinstance(self.cache_type_v, Unset):
            cache_type_v = UNSET
        else:
            cache_type_v = self.cache_type_v

        parallel_slots: int | None | Unset
        if isinstance(self.parallel_slots, Unset):
            parallel_slots = UNSET
        else:
            parallel_slots = self.parallel_slots

        logical_batch: int | None | Unset
        if isinstance(self.logical_batch, Unset):
            logical_batch = UNSET
        else:
            logical_batch = self.logical_batch

        physical_batch: int | None | Unset
        if isinstance(self.physical_batch, Unset):
            physical_batch = UNSET
        else:
            physical_batch = self.physical_batch

        context_shift: bool | None | Unset
        if isinstance(self.context_shift, Unset):
            context_shift = UNSET
        else:
            context_shift = self.context_shift

        split_mode: None | str | Unset
        if isinstance(self.split_mode, Unset):
            split_mode = UNSET
        else:
            split_mode = self.split_mode

        main_gpu: int | None | Unset
        if isinstance(self.main_gpu, Unset):
            main_gpu = UNSET
        else:
            main_gpu = self.main_gpu

        tensor_split: None | str | Unset
        if isinstance(self.tensor_split, Unset):
            tensor_split = UNSET
        else:
            tensor_split = self.tensor_split

        rope_scaling: None | str | Unset
        if isinstance(self.rope_scaling, Unset):
            rope_scaling = UNSET
        else:
            rope_scaling = self.rope_scaling

        rope_freq_base: float | None | Unset
        if isinstance(self.rope_freq_base, Unset):
            rope_freq_base = UNSET
        else:
            rope_freq_base = self.rope_freq_base

        rope_freq_scale: float | None | Unset
        if isinstance(self.rope_freq_scale, Unset):
            rope_freq_scale = UNSET
        else:
            rope_freq_scale = self.rope_freq_scale

        chat_template: None | str | Unset
        if isinstance(self.chat_template, Unset):
            chat_template = UNSET
        else:
            chat_template = self.chat_template

        api_key_ref: None | str | Unset
        if isinstance(self.api_key_ref, Unset):
            api_key_ref = UNSET
        else:
            api_key_ref = self.api_key_ref

        custom_args: None | str | Unset
        if isinstance(self.custom_args, Unset):
            custom_args = UNSET
        else:
            custom_args = self.custom_args

        draft_model_path: None | str | Unset
        if isinstance(self.draft_model_path, Unset):
            draft_model_path = UNSET
        else:
            draft_model_path = self.draft_model_path

        draft_gpu_layers: int | None | Unset
        if isinstance(self.draft_gpu_layers, Unset):
            draft_gpu_layers = UNSET
        else:
            draft_gpu_layers = self.draft_gpu_layers

        lora_path: None | str | Unset
        if isinstance(self.lora_path, Unset):
            lora_path = UNSET
        else:
            lora_path = self.lora_path

        lora_scale: float | None | Unset
        if isinstance(self.lora_scale, Unset):
            lora_scale = UNSET
        else:
            lora_scale = self.lora_scale

        mmproj_path: None | str | Unset
        if isinstance(self.mmproj_path, Unset):
            mmproj_path = UNSET
        else:
            mmproj_path = self.mmproj_path

        router_mode = self.router_mode

        models_path: None | str | Unset
        if isinstance(self.models_path, Unset):
            models_path = UNSET
        else:
            models_path = self.models_path

        mcp_enabled = self.mcp_enabled

        mcp_config_path: None | str | Unset
        if isinstance(self.mcp_config_path, Unset):
            mcp_config_path = UNSET
        else:
            mcp_config_path = self.mcp_config_path

        params: dict[str, Any] | Unset = UNSET
        if not isinstance(self.params, Unset):
            params = self.params.to_dict()

        runtime: None | str | Unset
        if isinstance(self.runtime, Unset):
            runtime = UNSET
        else:
            runtime = self.runtime

        alias: None | str | Unset
        if isinstance(self.alias, Unset):
            alias = UNSET
        else:
            alias = self.alias

        definition_id: None | str | Unset
        if isinstance(self.definition_id, Unset):
            definition_id = UNSET
        else:
            definition_id = self.definition_id

        launched_by: str | Unset = UNSET
        if not isinstance(self.launched_by, Unset):
            launched_by = self.launched_by

        auto_fields: list[str] | Unset = UNSET
        if not isinstance(self.auto_fields, Unset):
            auto_fields = self.auto_fields

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "model_path": model_path,
                "gpu_layers": gpu_layers,
                "context_size": context_size,
                "host": host,
                "port": port,
            }
        )
        if binary_path is not UNSET:
            field_dict["binary_path"] = binary_path
        if port_auto is not UNSET:
            field_dict["port_auto"] = port_auto
        if threads is not UNSET:
            field_dict["threads"] = threads
        if flash_attn is not UNSET:
            field_dict["flash_attn"] = flash_attn
        if cache_type_k is not UNSET:
            field_dict["cache_type_k"] = cache_type_k
        if cache_type_v is not UNSET:
            field_dict["cache_type_v"] = cache_type_v
        if parallel_slots is not UNSET:
            field_dict["parallel_slots"] = parallel_slots
        if logical_batch is not UNSET:
            field_dict["logical_batch"] = logical_batch
        if physical_batch is not UNSET:
            field_dict["physical_batch"] = physical_batch
        if context_shift is not UNSET:
            field_dict["context_shift"] = context_shift
        if split_mode is not UNSET:
            field_dict["split_mode"] = split_mode
        if main_gpu is not UNSET:
            field_dict["main_gpu"] = main_gpu
        if tensor_split is not UNSET:
            field_dict["tensor_split"] = tensor_split
        if rope_scaling is not UNSET:
            field_dict["rope_scaling"] = rope_scaling
        if rope_freq_base is not UNSET:
            field_dict["rope_freq_base"] = rope_freq_base
        if rope_freq_scale is not UNSET:
            field_dict["rope_freq_scale"] = rope_freq_scale
        if chat_template is not UNSET:
            field_dict["chat_template"] = chat_template
        if api_key_ref is not UNSET:
            field_dict["api_key_ref"] = api_key_ref
        if custom_args is not UNSET:
            field_dict["custom_args"] = custom_args
        if draft_model_path is not UNSET:
            field_dict["draft_model_path"] = draft_model_path
        if draft_gpu_layers is not UNSET:
            field_dict["draft_gpu_layers"] = draft_gpu_layers
        if lora_path is not UNSET:
            field_dict["lora_path"] = lora_path
        if lora_scale is not UNSET:
            field_dict["lora_scale"] = lora_scale
        if mmproj_path is not UNSET:
            field_dict["mmproj_path"] = mmproj_path
        if router_mode is not UNSET:
            field_dict["router_mode"] = router_mode
        if models_path is not UNSET:
            field_dict["models_path"] = models_path
        if mcp_enabled is not UNSET:
            field_dict["mcp_enabled"] = mcp_enabled
        if mcp_config_path is not UNSET:
            field_dict["mcp_config_path"] = mcp_config_path
        if params is not UNSET:
            field_dict["params"] = params
        if runtime is not UNSET:
            field_dict["runtime"] = runtime
        if alias is not UNSET:
            field_dict["alias"] = alias
        if definition_id is not UNSET:
            field_dict["definition_id"] = definition_id
        if launched_by is not UNSET:
            field_dict["launched_by"] = launched_by
        if auto_fields is not UNSET:
            field_dict["auto_fields"] = auto_fields

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.b_tree_map import BTreeMap  # noqa: PLC0415

        d = dict(src_dict)
        model_path = d.pop("model_path")

        gpu_layers = d.pop("gpu_layers")

        context_size = d.pop("context_size")

        host = d.pop("host")

        port = d.pop("port")

        binary_path = d.pop("binary_path", UNSET)

        port_auto = d.pop("port_auto", UNSET)

        def _parse_threads(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        threads = _parse_threads(d.pop("threads", UNSET))

        flash_attn = d.pop("flash_attn", UNSET)

        def _parse_cache_type_k(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cache_type_k = _parse_cache_type_k(d.pop("cache_type_k", UNSET))

        def _parse_cache_type_v(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        cache_type_v = _parse_cache_type_v(d.pop("cache_type_v", UNSET))

        def _parse_parallel_slots(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        parallel_slots = _parse_parallel_slots(d.pop("parallel_slots", UNSET))

        def _parse_logical_batch(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        logical_batch = _parse_logical_batch(d.pop("logical_batch", UNSET))

        def _parse_physical_batch(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        physical_batch = _parse_physical_batch(d.pop("physical_batch", UNSET))

        def _parse_context_shift(data: object) -> bool | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(bool | None | Unset, data)

        context_shift = _parse_context_shift(d.pop("context_shift", UNSET))

        def _parse_split_mode(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        split_mode = _parse_split_mode(d.pop("split_mode", UNSET))

        def _parse_main_gpu(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        main_gpu = _parse_main_gpu(d.pop("main_gpu", UNSET))

        def _parse_tensor_split(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        tensor_split = _parse_tensor_split(d.pop("tensor_split", UNSET))

        def _parse_rope_scaling(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        rope_scaling = _parse_rope_scaling(d.pop("rope_scaling", UNSET))

        def _parse_rope_freq_base(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        rope_freq_base = _parse_rope_freq_base(d.pop("rope_freq_base", UNSET))

        def _parse_rope_freq_scale(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        rope_freq_scale = _parse_rope_freq_scale(d.pop("rope_freq_scale", UNSET))

        def _parse_chat_template(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        chat_template = _parse_chat_template(d.pop("chat_template", UNSET))

        def _parse_api_key_ref(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        api_key_ref = _parse_api_key_ref(d.pop("api_key_ref", UNSET))

        def _parse_custom_args(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        custom_args = _parse_custom_args(d.pop("custom_args", UNSET))

        def _parse_draft_model_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        draft_model_path = _parse_draft_model_path(d.pop("draft_model_path", UNSET))

        def _parse_draft_gpu_layers(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        draft_gpu_layers = _parse_draft_gpu_layers(d.pop("draft_gpu_layers", UNSET))

        def _parse_lora_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        lora_path = _parse_lora_path(d.pop("lora_path", UNSET))

        def _parse_lora_scale(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        lora_scale = _parse_lora_scale(d.pop("lora_scale", UNSET))

        def _parse_mmproj_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mmproj_path = _parse_mmproj_path(d.pop("mmproj_path", UNSET))

        router_mode = d.pop("router_mode", UNSET)

        def _parse_models_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        models_path = _parse_models_path(d.pop("models_path", UNSET))

        mcp_enabled = d.pop("mcp_enabled", UNSET)

        def _parse_mcp_config_path(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mcp_config_path = _parse_mcp_config_path(d.pop("mcp_config_path", UNSET))

        _params = d.pop("params", UNSET)
        params: BTreeMap | Unset
        if isinstance(_params, Unset):
            params = UNSET
        else:
            params = BTreeMap.from_dict(_params)

        def _parse_runtime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        runtime = _parse_runtime(d.pop("runtime", UNSET))

        def _parse_alias(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        alias = _parse_alias(d.pop("alias", UNSET))

        def _parse_definition_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        definition_id = _parse_definition_id(d.pop("definition_id", UNSET))

        _launched_by = d.pop("launched_by", UNSET)
        launched_by: SlotOrigin | Unset
        if isinstance(_launched_by, Unset):
            launched_by = UNSET
        else:
            launched_by = check_slot_origin(_launched_by)

        auto_fields = cast(list[str], d.pop("auto_fields", UNSET))

        launch_config = cls(
            model_path=model_path,
            gpu_layers=gpu_layers,
            context_size=context_size,
            host=host,
            port=port,
            binary_path=binary_path,
            port_auto=port_auto,
            threads=threads,
            flash_attn=flash_attn,
            cache_type_k=cache_type_k,
            cache_type_v=cache_type_v,
            parallel_slots=parallel_slots,
            logical_batch=logical_batch,
            physical_batch=physical_batch,
            context_shift=context_shift,
            split_mode=split_mode,
            main_gpu=main_gpu,
            tensor_split=tensor_split,
            rope_scaling=rope_scaling,
            rope_freq_base=rope_freq_base,
            rope_freq_scale=rope_freq_scale,
            chat_template=chat_template,
            api_key_ref=api_key_ref,
            custom_args=custom_args,
            draft_model_path=draft_model_path,
            draft_gpu_layers=draft_gpu_layers,
            lora_path=lora_path,
            lora_scale=lora_scale,
            mmproj_path=mmproj_path,
            router_mode=router_mode,
            models_path=models_path,
            mcp_enabled=mcp_enabled,
            mcp_config_path=mcp_config_path,
            params=params,
            runtime=runtime,
            alias=alias,
            definition_id=definition_id,
            launched_by=launched_by,
            auto_fields=auto_fields,
        )

        launch_config.additional_properties = d
        return launch_config

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
