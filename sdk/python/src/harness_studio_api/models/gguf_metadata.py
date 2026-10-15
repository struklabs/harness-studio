from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.reasoning_control import ReasoningControl
    from ..models.tokenizer_info import TokenizerInfo


T = TypeVar("T", bound="GgufMetadata")


@_attrs_define
class GgufMetadata:
    """
    Attributes:
        version (int):
        tensor_count (int):
        kv_count (int):
        quantization (str):
        architecture (None | str | Unset):
        audiocpp_family (None | str | Unset): audio.cpp model family; only meaningful for architecture = audiocpp.
        model_name (None | str | Unset):
        url (None | str | Unset): Provenance URLs that may identify a Hugging Face repository when a split file was
            copied into the library by hand.
        repo_url (None | str | Unset):
        source_url (None | str | Unset):
        source_repo_url (None | str | Unset):
        context_length (int | None | Unset):
        file_type (int | None | Unset):
        block_count (int | None | Unset):
        embedding_length (int | None | Unset):
        head_count (int | None | Unset):
        head_count_kv (int | None | Unset):
        key_length (int | None | Unset):
        value_length (int | None | Unset):
        expert_count (int | None | Unset):
        expert_used_count (int | None | Unset):
        feed_forward_length (int | None | Unset): `<arch>.feed_forward_length` (largest value of per-layer arrays).
        expert_feed_forward_length (int | None | Unset): `<arch>.expert_feed_forward_length` (MoE experts).
        sliding_window (int | None | Unset): `<arch>.attention.sliding_window`: window of sliding-window-attention
            layers (tokens).
        sliding_window_pattern (int | None | Unset): `<arch>.attention.sliding_window_pattern` as a number: every N-th
            layer is a full
            attention layer, the others use the window (llama.cpp `set_swa_pattern(N)`).
        swa_layer_count (int | None | Unset): `<arch>.attention.sliding_window_pattern` as a per-layer array: number of
            layers that
            use the window.
        key_length_swa (int | None | Unset): `<arch>.attention.key_length_swa` / `value_length_swa`: head size of the
            sliding-window layers when it differs from the full-attention layers (Gemma 4).
        value_length_swa (int | None | Unset):
        head_count_kv_swa (int | None | Unset): KV heads of the sliding-window and of the full-attention layers, from
            the per-layer
            `head_count_kv` and `sliding_window_pattern` arrays (Gemma 4: 8 and 1). `None` when
            the file has no per-layer arrays (then `head_count_kv` applies to every layer).
        head_count_kv_full (int | None | Unset):
        attention_layer_count (int | None | Unset): Layers with attention (a KV cache) of hybrid models whose per-layer
            `head_count_kv`
            array is 0 for the recurrent layers (Jamba, Granite hybrid, Nemotron-H).
        full_attention_interval (int | None | Unset): `<arch>.full_attention_interval`: every N-th layer has attention,
            the others are
            recurrent (Qwen3-Next, Qwen3.5/3.6).
        ssm_conv_kernel (int | None | Unset): `<arch>.ssm.*` of recurrent / hybrid models (Mamba, gated delta net): they
            keep one
            fixed-size state per sequence (= per server slot).
        ssm_state_size (int | None | Unset):
        ssm_inner_size (int | None | Unset):
        ssm_group_count (int | None | Unset):
        rope_scaling_type (None | str | Unset): `<arch>.rope.scaling.type` (`yarn`, `linear`, `none`...): RoPE scaling
            the file declares.
        rope_scaling_factor (float | None | Unset): `<arch>.rope.scaling.factor`.
        rope_scaling_original_ctx (int | None | Unset): `<arch>.rope.scaling.original_context_length`: context the model
            was trained on before
            the scaling.
        token_embd_bytes (int | None | Unset): Bytes of `token_embd.weight` (llama.cpp keeps it in host memory for the
            lookup).
        output_bytes (int | None | Unset): Bytes of `output.weight`; `None` with `token_embd_bytes` set = tied output
            (llama.cpp
            then also uploads a copy of the embeddings as the output layer).
        has_chat_template (bool | Unset):
        template_supports_tools (bool | Unset): The embedded chat template handles `tools` (native tool/function
            calling).
        pooling_type (int | None | Unset): Tokenizer / pooling hints for embedding & reranker models.
        is_mmproj (bool | Unset): True for multimodal projector files (`general.type = mmproj` / clip architecture).
        general_type (None | str | Unset): `general.type` ("model", "mmproj", "adapter", "imatrix"...).
        adapter_type (None | str | Unset): `adapter.type` of adapter files ("lora").
        has_vision_encoder (bool | Unset): `clip.has_vision_encoder` / `clip.has_audio_encoder` of projector files.
        has_audio_encoder (bool | Unset):
        has_classifier_labels (bool | Unset): A `<arch>.classifier.output_labels` key exists (classification heads, e.g.
            rerankers).
        has_cls_head (bool | Unset): A sequence-classification head (`cls.output.weight` / `cls.weight` tensors), which
            is
            how BERT/XLM-R rerankers such as bge-reranker-v2-m3 are stored (no `pooling_type` key).
        size_label (None | str | Unset): `general.size_label` ("4B", "567M", "32x2.4B"; free text, display only).
        basename (None | str | Unset): `general.basename` (pairs a projector with its model: both carry the same
            basename).
        param_count (int | None | Unset): Sum of the element counts of the tensors described in this file (exact; for
            split
            models only this shard's tensors). `None` when the tensor table could not be read.
        split_count (int | None | Unset): `split.count` of split GGUFs (0 or absent for single files).
        template_supports_reasoning (bool | Unset): The embedded chat template can switch reasoning on
            (`enable_thinking`,
            `reasoning_effort`, gpt-oss channels, or an unconditional `<think>` opener).
        reasoning_control (ReasoningControl | Unset):
        tokenizer (None | TokenizerInfo | Unset):
        tensor_types (list[int] | Unset): Distinct ggml tensor types of this file (numeric `ggml_type` ids, sorted).
            Used to tell
            whether a runtime can load the file (fork-only quant types; see `runtimes::quant`).
        legacy_prism_q2_0 (bool | Unset): Type 42 whose byte spans match Prism's deprecated group-128 layout rather than
            upstream Q2_0's group-64 layout. Only set when the tensor table proves the difference.
    """

    version: int
    tensor_count: int
    kv_count: int
    quantization: str
    architecture: None | str | Unset = UNSET
    audiocpp_family: None | str | Unset = UNSET
    model_name: None | str | Unset = UNSET
    url: None | str | Unset = UNSET
    repo_url: None | str | Unset = UNSET
    source_url: None | str | Unset = UNSET
    source_repo_url: None | str | Unset = UNSET
    context_length: int | None | Unset = UNSET
    file_type: int | None | Unset = UNSET
    block_count: int | None | Unset = UNSET
    embedding_length: int | None | Unset = UNSET
    head_count: int | None | Unset = UNSET
    head_count_kv: int | None | Unset = UNSET
    key_length: int | None | Unset = UNSET
    value_length: int | None | Unset = UNSET
    expert_count: int | None | Unset = UNSET
    expert_used_count: int | None | Unset = UNSET
    feed_forward_length: int | None | Unset = UNSET
    expert_feed_forward_length: int | None | Unset = UNSET
    sliding_window: int | None | Unset = UNSET
    sliding_window_pattern: int | None | Unset = UNSET
    swa_layer_count: int | None | Unset = UNSET
    key_length_swa: int | None | Unset = UNSET
    value_length_swa: int | None | Unset = UNSET
    head_count_kv_swa: int | None | Unset = UNSET
    head_count_kv_full: int | None | Unset = UNSET
    attention_layer_count: int | None | Unset = UNSET
    full_attention_interval: int | None | Unset = UNSET
    ssm_conv_kernel: int | None | Unset = UNSET
    ssm_state_size: int | None | Unset = UNSET
    ssm_inner_size: int | None | Unset = UNSET
    ssm_group_count: int | None | Unset = UNSET
    rope_scaling_type: None | str | Unset = UNSET
    rope_scaling_factor: float | None | Unset = UNSET
    rope_scaling_original_ctx: int | None | Unset = UNSET
    token_embd_bytes: int | None | Unset = UNSET
    output_bytes: int | None | Unset = UNSET
    has_chat_template: bool | Unset = UNSET
    template_supports_tools: bool | Unset = UNSET
    pooling_type: int | None | Unset = UNSET
    is_mmproj: bool | Unset = UNSET
    general_type: None | str | Unset = UNSET
    adapter_type: None | str | Unset = UNSET
    has_vision_encoder: bool | Unset = UNSET
    has_audio_encoder: bool | Unset = UNSET
    has_classifier_labels: bool | Unset = UNSET
    has_cls_head: bool | Unset = UNSET
    size_label: None | str | Unset = UNSET
    basename: None | str | Unset = UNSET
    param_count: int | None | Unset = UNSET
    split_count: int | None | Unset = UNSET
    template_supports_reasoning: bool | Unset = UNSET
    reasoning_control: ReasoningControl | Unset = UNSET
    tokenizer: None | TokenizerInfo | Unset = UNSET
    tensor_types: list[int] | Unset = UNSET
    legacy_prism_q2_0: bool | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.tokenizer_info import TokenizerInfo  # noqa: PLC0415

        version = self.version

        tensor_count = self.tensor_count

        kv_count = self.kv_count

        quantization = self.quantization

        architecture: None | str | Unset
        if isinstance(self.architecture, Unset):
            architecture = UNSET
        else:
            architecture = self.architecture

        audiocpp_family: None | str | Unset
        if isinstance(self.audiocpp_family, Unset):
            audiocpp_family = UNSET
        else:
            audiocpp_family = self.audiocpp_family

        model_name: None | str | Unset
        if isinstance(self.model_name, Unset):
            model_name = UNSET
        else:
            model_name = self.model_name

        url: None | str | Unset
        if isinstance(self.url, Unset):
            url = UNSET
        else:
            url = self.url

        repo_url: None | str | Unset
        if isinstance(self.repo_url, Unset):
            repo_url = UNSET
        else:
            repo_url = self.repo_url

        source_url: None | str | Unset
        if isinstance(self.source_url, Unset):
            source_url = UNSET
        else:
            source_url = self.source_url

        source_repo_url: None | str | Unset
        if isinstance(self.source_repo_url, Unset):
            source_repo_url = UNSET
        else:
            source_repo_url = self.source_repo_url

        context_length: int | None | Unset
        if isinstance(self.context_length, Unset):
            context_length = UNSET
        else:
            context_length = self.context_length

        file_type: int | None | Unset
        if isinstance(self.file_type, Unset):
            file_type = UNSET
        else:
            file_type = self.file_type

        block_count: int | None | Unset
        if isinstance(self.block_count, Unset):
            block_count = UNSET
        else:
            block_count = self.block_count

        embedding_length: int | None | Unset
        if isinstance(self.embedding_length, Unset):
            embedding_length = UNSET
        else:
            embedding_length = self.embedding_length

        head_count: int | None | Unset
        if isinstance(self.head_count, Unset):
            head_count = UNSET
        else:
            head_count = self.head_count

        head_count_kv: int | None | Unset
        if isinstance(self.head_count_kv, Unset):
            head_count_kv = UNSET
        else:
            head_count_kv = self.head_count_kv

        key_length: int | None | Unset
        if isinstance(self.key_length, Unset):
            key_length = UNSET
        else:
            key_length = self.key_length

        value_length: int | None | Unset
        if isinstance(self.value_length, Unset):
            value_length = UNSET
        else:
            value_length = self.value_length

        expert_count: int | None | Unset
        if isinstance(self.expert_count, Unset):
            expert_count = UNSET
        else:
            expert_count = self.expert_count

        expert_used_count: int | None | Unset
        if isinstance(self.expert_used_count, Unset):
            expert_used_count = UNSET
        else:
            expert_used_count = self.expert_used_count

        feed_forward_length: int | None | Unset
        if isinstance(self.feed_forward_length, Unset):
            feed_forward_length = UNSET
        else:
            feed_forward_length = self.feed_forward_length

        expert_feed_forward_length: int | None | Unset
        if isinstance(self.expert_feed_forward_length, Unset):
            expert_feed_forward_length = UNSET
        else:
            expert_feed_forward_length = self.expert_feed_forward_length

        sliding_window: int | None | Unset
        if isinstance(self.sliding_window, Unset):
            sliding_window = UNSET
        else:
            sliding_window = self.sliding_window

        sliding_window_pattern: int | None | Unset
        if isinstance(self.sliding_window_pattern, Unset):
            sliding_window_pattern = UNSET
        else:
            sliding_window_pattern = self.sliding_window_pattern

        swa_layer_count: int | None | Unset
        if isinstance(self.swa_layer_count, Unset):
            swa_layer_count = UNSET
        else:
            swa_layer_count = self.swa_layer_count

        key_length_swa: int | None | Unset
        if isinstance(self.key_length_swa, Unset):
            key_length_swa = UNSET
        else:
            key_length_swa = self.key_length_swa

        value_length_swa: int | None | Unset
        if isinstance(self.value_length_swa, Unset):
            value_length_swa = UNSET
        else:
            value_length_swa = self.value_length_swa

        head_count_kv_swa: int | None | Unset
        if isinstance(self.head_count_kv_swa, Unset):
            head_count_kv_swa = UNSET
        else:
            head_count_kv_swa = self.head_count_kv_swa

        head_count_kv_full: int | None | Unset
        if isinstance(self.head_count_kv_full, Unset):
            head_count_kv_full = UNSET
        else:
            head_count_kv_full = self.head_count_kv_full

        attention_layer_count: int | None | Unset
        if isinstance(self.attention_layer_count, Unset):
            attention_layer_count = UNSET
        else:
            attention_layer_count = self.attention_layer_count

        full_attention_interval: int | None | Unset
        if isinstance(self.full_attention_interval, Unset):
            full_attention_interval = UNSET
        else:
            full_attention_interval = self.full_attention_interval

        ssm_conv_kernel: int | None | Unset
        if isinstance(self.ssm_conv_kernel, Unset):
            ssm_conv_kernel = UNSET
        else:
            ssm_conv_kernel = self.ssm_conv_kernel

        ssm_state_size: int | None | Unset
        if isinstance(self.ssm_state_size, Unset):
            ssm_state_size = UNSET
        else:
            ssm_state_size = self.ssm_state_size

        ssm_inner_size: int | None | Unset
        if isinstance(self.ssm_inner_size, Unset):
            ssm_inner_size = UNSET
        else:
            ssm_inner_size = self.ssm_inner_size

        ssm_group_count: int | None | Unset
        if isinstance(self.ssm_group_count, Unset):
            ssm_group_count = UNSET
        else:
            ssm_group_count = self.ssm_group_count

        rope_scaling_type: None | str | Unset
        if isinstance(self.rope_scaling_type, Unset):
            rope_scaling_type = UNSET
        else:
            rope_scaling_type = self.rope_scaling_type

        rope_scaling_factor: float | None | Unset
        if isinstance(self.rope_scaling_factor, Unset):
            rope_scaling_factor = UNSET
        else:
            rope_scaling_factor = self.rope_scaling_factor

        rope_scaling_original_ctx: int | None | Unset
        if isinstance(self.rope_scaling_original_ctx, Unset):
            rope_scaling_original_ctx = UNSET
        else:
            rope_scaling_original_ctx = self.rope_scaling_original_ctx

        token_embd_bytes: int | None | Unset
        if isinstance(self.token_embd_bytes, Unset):
            token_embd_bytes = UNSET
        else:
            token_embd_bytes = self.token_embd_bytes

        output_bytes: int | None | Unset
        if isinstance(self.output_bytes, Unset):
            output_bytes = UNSET
        else:
            output_bytes = self.output_bytes

        has_chat_template = self.has_chat_template

        template_supports_tools = self.template_supports_tools

        pooling_type: int | None | Unset
        if isinstance(self.pooling_type, Unset):
            pooling_type = UNSET
        else:
            pooling_type = self.pooling_type

        is_mmproj = self.is_mmproj

        general_type: None | str | Unset
        if isinstance(self.general_type, Unset):
            general_type = UNSET
        else:
            general_type = self.general_type

        adapter_type: None | str | Unset
        if isinstance(self.adapter_type, Unset):
            adapter_type = UNSET
        else:
            adapter_type = self.adapter_type

        has_vision_encoder = self.has_vision_encoder

        has_audio_encoder = self.has_audio_encoder

        has_classifier_labels = self.has_classifier_labels

        has_cls_head = self.has_cls_head

        size_label: None | str | Unset
        if isinstance(self.size_label, Unset):
            size_label = UNSET
        else:
            size_label = self.size_label

        basename: None | str | Unset
        if isinstance(self.basename, Unset):
            basename = UNSET
        else:
            basename = self.basename

        param_count: int | None | Unset
        if isinstance(self.param_count, Unset):
            param_count = UNSET
        else:
            param_count = self.param_count

        split_count: int | None | Unset
        if isinstance(self.split_count, Unset):
            split_count = UNSET
        else:
            split_count = self.split_count

        template_supports_reasoning = self.template_supports_reasoning

        reasoning_control: dict[str, Any] | Unset = UNSET
        if not isinstance(self.reasoning_control, Unset):
            reasoning_control = self.reasoning_control.to_dict()

        tokenizer: dict[str, Any] | None | Unset
        if isinstance(self.tokenizer, Unset):
            tokenizer = UNSET
        elif isinstance(self.tokenizer, TokenizerInfo):
            tokenizer = self.tokenizer.to_dict()
        else:
            tokenizer = self.tokenizer

        tensor_types: list[int] | Unset = UNSET
        if not isinstance(self.tensor_types, Unset):
            tensor_types = self.tensor_types

        legacy_prism_q2_0 = self.legacy_prism_q2_0

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "version": version,
                "tensor_count": tensor_count,
                "kv_count": kv_count,
                "quantization": quantization,
            }
        )
        if architecture is not UNSET:
            field_dict["architecture"] = architecture
        if audiocpp_family is not UNSET:
            field_dict["audiocpp_family"] = audiocpp_family
        if model_name is not UNSET:
            field_dict["model_name"] = model_name
        if url is not UNSET:
            field_dict["url"] = url
        if repo_url is not UNSET:
            field_dict["repo_url"] = repo_url
        if source_url is not UNSET:
            field_dict["source_url"] = source_url
        if source_repo_url is not UNSET:
            field_dict["source_repo_url"] = source_repo_url
        if context_length is not UNSET:
            field_dict["context_length"] = context_length
        if file_type is not UNSET:
            field_dict["file_type"] = file_type
        if block_count is not UNSET:
            field_dict["block_count"] = block_count
        if embedding_length is not UNSET:
            field_dict["embedding_length"] = embedding_length
        if head_count is not UNSET:
            field_dict["head_count"] = head_count
        if head_count_kv is not UNSET:
            field_dict["head_count_kv"] = head_count_kv
        if key_length is not UNSET:
            field_dict["key_length"] = key_length
        if value_length is not UNSET:
            field_dict["value_length"] = value_length
        if expert_count is not UNSET:
            field_dict["expert_count"] = expert_count
        if expert_used_count is not UNSET:
            field_dict["expert_used_count"] = expert_used_count
        if feed_forward_length is not UNSET:
            field_dict["feed_forward_length"] = feed_forward_length
        if expert_feed_forward_length is not UNSET:
            field_dict["expert_feed_forward_length"] = expert_feed_forward_length
        if sliding_window is not UNSET:
            field_dict["sliding_window"] = sliding_window
        if sliding_window_pattern is not UNSET:
            field_dict["sliding_window_pattern"] = sliding_window_pattern
        if swa_layer_count is not UNSET:
            field_dict["swa_layer_count"] = swa_layer_count
        if key_length_swa is not UNSET:
            field_dict["key_length_swa"] = key_length_swa
        if value_length_swa is not UNSET:
            field_dict["value_length_swa"] = value_length_swa
        if head_count_kv_swa is not UNSET:
            field_dict["head_count_kv_swa"] = head_count_kv_swa
        if head_count_kv_full is not UNSET:
            field_dict["head_count_kv_full"] = head_count_kv_full
        if attention_layer_count is not UNSET:
            field_dict["attention_layer_count"] = attention_layer_count
        if full_attention_interval is not UNSET:
            field_dict["full_attention_interval"] = full_attention_interval
        if ssm_conv_kernel is not UNSET:
            field_dict["ssm_conv_kernel"] = ssm_conv_kernel
        if ssm_state_size is not UNSET:
            field_dict["ssm_state_size"] = ssm_state_size
        if ssm_inner_size is not UNSET:
            field_dict["ssm_inner_size"] = ssm_inner_size
        if ssm_group_count is not UNSET:
            field_dict["ssm_group_count"] = ssm_group_count
        if rope_scaling_type is not UNSET:
            field_dict["rope_scaling_type"] = rope_scaling_type
        if rope_scaling_factor is not UNSET:
            field_dict["rope_scaling_factor"] = rope_scaling_factor
        if rope_scaling_original_ctx is not UNSET:
            field_dict["rope_scaling_original_ctx"] = rope_scaling_original_ctx
        if token_embd_bytes is not UNSET:
            field_dict["token_embd_bytes"] = token_embd_bytes
        if output_bytes is not UNSET:
            field_dict["output_bytes"] = output_bytes
        if has_chat_template is not UNSET:
            field_dict["has_chat_template"] = has_chat_template
        if template_supports_tools is not UNSET:
            field_dict["template_supports_tools"] = template_supports_tools
        if pooling_type is not UNSET:
            field_dict["pooling_type"] = pooling_type
        if is_mmproj is not UNSET:
            field_dict["is_mmproj"] = is_mmproj
        if general_type is not UNSET:
            field_dict["general_type"] = general_type
        if adapter_type is not UNSET:
            field_dict["adapter_type"] = adapter_type
        if has_vision_encoder is not UNSET:
            field_dict["has_vision_encoder"] = has_vision_encoder
        if has_audio_encoder is not UNSET:
            field_dict["has_audio_encoder"] = has_audio_encoder
        if has_classifier_labels is not UNSET:
            field_dict["has_classifier_labels"] = has_classifier_labels
        if has_cls_head is not UNSET:
            field_dict["has_cls_head"] = has_cls_head
        if size_label is not UNSET:
            field_dict["size_label"] = size_label
        if basename is not UNSET:
            field_dict["basename"] = basename
        if param_count is not UNSET:
            field_dict["param_count"] = param_count
        if split_count is not UNSET:
            field_dict["split_count"] = split_count
        if template_supports_reasoning is not UNSET:
            field_dict["template_supports_reasoning"] = template_supports_reasoning
        if reasoning_control is not UNSET:
            field_dict["reasoning_control"] = reasoning_control
        if tokenizer is not UNSET:
            field_dict["tokenizer"] = tokenizer
        if tensor_types is not UNSET:
            field_dict["tensor_types"] = tensor_types
        if legacy_prism_q2_0 is not UNSET:
            field_dict["legacy_prism_q2_0"] = legacy_prism_q2_0

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.reasoning_control import ReasoningControl  # noqa: PLC0415
        from ..models.tokenizer_info import TokenizerInfo  # noqa: PLC0415

        d = dict(src_dict)
        version = d.pop("version")

        tensor_count = d.pop("tensor_count")

        kv_count = d.pop("kv_count")

        quantization = d.pop("quantization")

        def _parse_architecture(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        architecture = _parse_architecture(d.pop("architecture", UNSET))

        def _parse_audiocpp_family(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        audiocpp_family = _parse_audiocpp_family(d.pop("audiocpp_family", UNSET))

        def _parse_model_name(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        model_name = _parse_model_name(d.pop("model_name", UNSET))

        def _parse_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        url = _parse_url(d.pop("url", UNSET))

        def _parse_repo_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        repo_url = _parse_repo_url(d.pop("repo_url", UNSET))

        def _parse_source_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source_url = _parse_source_url(d.pop("source_url", UNSET))

        def _parse_source_repo_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        source_repo_url = _parse_source_repo_url(d.pop("source_repo_url", UNSET))

        def _parse_context_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        context_length = _parse_context_length(d.pop("context_length", UNSET))

        def _parse_file_type(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        file_type = _parse_file_type(d.pop("file_type", UNSET))

        def _parse_block_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        block_count = _parse_block_count(d.pop("block_count", UNSET))

        def _parse_embedding_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        embedding_length = _parse_embedding_length(d.pop("embedding_length", UNSET))

        def _parse_head_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        head_count = _parse_head_count(d.pop("head_count", UNSET))

        def _parse_head_count_kv(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        head_count_kv = _parse_head_count_kv(d.pop("head_count_kv", UNSET))

        def _parse_key_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        key_length = _parse_key_length(d.pop("key_length", UNSET))

        def _parse_value_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        value_length = _parse_value_length(d.pop("value_length", UNSET))

        def _parse_expert_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expert_count = _parse_expert_count(d.pop("expert_count", UNSET))

        def _parse_expert_used_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expert_used_count = _parse_expert_used_count(d.pop("expert_used_count", UNSET))

        def _parse_feed_forward_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        feed_forward_length = _parse_feed_forward_length(d.pop("feed_forward_length", UNSET))

        def _parse_expert_feed_forward_length(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        expert_feed_forward_length = _parse_expert_feed_forward_length(
            d.pop("expert_feed_forward_length", UNSET)
        )

        def _parse_sliding_window(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        sliding_window = _parse_sliding_window(d.pop("sliding_window", UNSET))

        def _parse_sliding_window_pattern(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        sliding_window_pattern = _parse_sliding_window_pattern(
            d.pop("sliding_window_pattern", UNSET)
        )

        def _parse_swa_layer_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        swa_layer_count = _parse_swa_layer_count(d.pop("swa_layer_count", UNSET))

        def _parse_key_length_swa(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        key_length_swa = _parse_key_length_swa(d.pop("key_length_swa", UNSET))

        def _parse_value_length_swa(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        value_length_swa = _parse_value_length_swa(d.pop("value_length_swa", UNSET))

        def _parse_head_count_kv_swa(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        head_count_kv_swa = _parse_head_count_kv_swa(d.pop("head_count_kv_swa", UNSET))

        def _parse_head_count_kv_full(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        head_count_kv_full = _parse_head_count_kv_full(d.pop("head_count_kv_full", UNSET))

        def _parse_attention_layer_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        attention_layer_count = _parse_attention_layer_count(d.pop("attention_layer_count", UNSET))

        def _parse_full_attention_interval(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        full_attention_interval = _parse_full_attention_interval(
            d.pop("full_attention_interval", UNSET)
        )

        def _parse_ssm_conv_kernel(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ssm_conv_kernel = _parse_ssm_conv_kernel(d.pop("ssm_conv_kernel", UNSET))

        def _parse_ssm_state_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ssm_state_size = _parse_ssm_state_size(d.pop("ssm_state_size", UNSET))

        def _parse_ssm_inner_size(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ssm_inner_size = _parse_ssm_inner_size(d.pop("ssm_inner_size", UNSET))

        def _parse_ssm_group_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        ssm_group_count = _parse_ssm_group_count(d.pop("ssm_group_count", UNSET))

        def _parse_rope_scaling_type(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        rope_scaling_type = _parse_rope_scaling_type(d.pop("rope_scaling_type", UNSET))

        def _parse_rope_scaling_factor(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        rope_scaling_factor = _parse_rope_scaling_factor(d.pop("rope_scaling_factor", UNSET))

        def _parse_rope_scaling_original_ctx(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        rope_scaling_original_ctx = _parse_rope_scaling_original_ctx(
            d.pop("rope_scaling_original_ctx", UNSET)
        )

        def _parse_token_embd_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        token_embd_bytes = _parse_token_embd_bytes(d.pop("token_embd_bytes", UNSET))

        def _parse_output_bytes(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        output_bytes = _parse_output_bytes(d.pop("output_bytes", UNSET))

        has_chat_template = d.pop("has_chat_template", UNSET)

        template_supports_tools = d.pop("template_supports_tools", UNSET)

        def _parse_pooling_type(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        pooling_type = _parse_pooling_type(d.pop("pooling_type", UNSET))

        is_mmproj = d.pop("is_mmproj", UNSET)

        def _parse_general_type(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        general_type = _parse_general_type(d.pop("general_type", UNSET))

        def _parse_adapter_type(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        adapter_type = _parse_adapter_type(d.pop("adapter_type", UNSET))

        has_vision_encoder = d.pop("has_vision_encoder", UNSET)

        has_audio_encoder = d.pop("has_audio_encoder", UNSET)

        has_classifier_labels = d.pop("has_classifier_labels", UNSET)

        has_cls_head = d.pop("has_cls_head", UNSET)

        def _parse_size_label(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        size_label = _parse_size_label(d.pop("size_label", UNSET))

        def _parse_basename(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        basename = _parse_basename(d.pop("basename", UNSET))

        def _parse_param_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        param_count = _parse_param_count(d.pop("param_count", UNSET))

        def _parse_split_count(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        split_count = _parse_split_count(d.pop("split_count", UNSET))

        template_supports_reasoning = d.pop("template_supports_reasoning", UNSET)

        _reasoning_control = d.pop("reasoning_control", UNSET)
        reasoning_control: ReasoningControl | Unset
        if isinstance(_reasoning_control, Unset):
            reasoning_control = UNSET
        else:
            reasoning_control = ReasoningControl.from_dict(_reasoning_control)

        def _parse_tokenizer(data: object) -> None | TokenizerInfo | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                tokenizer_type_1 = TokenizerInfo.from_dict(data)

                return tokenizer_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | TokenizerInfo | Unset, data)

        tokenizer = _parse_tokenizer(d.pop("tokenizer", UNSET))

        tensor_types = cast(list[int], d.pop("tensor_types", UNSET))

        legacy_prism_q2_0 = d.pop("legacy_prism_q2_0", UNSET)

        gguf_metadata = cls(
            version=version,
            tensor_count=tensor_count,
            kv_count=kv_count,
            quantization=quantization,
            architecture=architecture,
            audiocpp_family=audiocpp_family,
            model_name=model_name,
            url=url,
            repo_url=repo_url,
            source_url=source_url,
            source_repo_url=source_repo_url,
            context_length=context_length,
            file_type=file_type,
            block_count=block_count,
            embedding_length=embedding_length,
            head_count=head_count,
            head_count_kv=head_count_kv,
            key_length=key_length,
            value_length=value_length,
            expert_count=expert_count,
            expert_used_count=expert_used_count,
            feed_forward_length=feed_forward_length,
            expert_feed_forward_length=expert_feed_forward_length,
            sliding_window=sliding_window,
            sliding_window_pattern=sliding_window_pattern,
            swa_layer_count=swa_layer_count,
            key_length_swa=key_length_swa,
            value_length_swa=value_length_swa,
            head_count_kv_swa=head_count_kv_swa,
            head_count_kv_full=head_count_kv_full,
            attention_layer_count=attention_layer_count,
            full_attention_interval=full_attention_interval,
            ssm_conv_kernel=ssm_conv_kernel,
            ssm_state_size=ssm_state_size,
            ssm_inner_size=ssm_inner_size,
            ssm_group_count=ssm_group_count,
            rope_scaling_type=rope_scaling_type,
            rope_scaling_factor=rope_scaling_factor,
            rope_scaling_original_ctx=rope_scaling_original_ctx,
            token_embd_bytes=token_embd_bytes,
            output_bytes=output_bytes,
            has_chat_template=has_chat_template,
            template_supports_tools=template_supports_tools,
            pooling_type=pooling_type,
            is_mmproj=is_mmproj,
            general_type=general_type,
            adapter_type=adapter_type,
            has_vision_encoder=has_vision_encoder,
            has_audio_encoder=has_audio_encoder,
            has_classifier_labels=has_classifier_labels,
            has_cls_head=has_cls_head,
            size_label=size_label,
            basename=basename,
            param_count=param_count,
            split_count=split_count,
            template_supports_reasoning=template_supports_reasoning,
            reasoning_control=reasoning_control,
            tokenizer=tokenizer,
            tensor_types=tensor_types,
            legacy_prism_q2_0=legacy_prism_q2_0,
        )

        gguf_metadata.additional_properties = d
        return gguf_metadata

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
