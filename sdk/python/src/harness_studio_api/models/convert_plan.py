from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.plan_file import PlanFile
    from ..models.quant_estimate import QuantEstimate


T = TypeVar("T", bound="ConvertPlan")


@_attrs_define
class ConvertPlan:
    """
    Attributes:
        repo_id (str):
        gated (bool):
        convertible (bool): The converter takes this repo.
        vision (bool): The converter also writes a vision projector for it (image-text-to-text).
        files (list[PlanFile]):
        download_bytes (int):
        parameters (int): Parameter count (from the Hub's safetensors header summary, else estimated).
        quants (list[QuantEstimate]):
        default_quant (str):
        free_bytes (int): Free bytes on the drive of the model library.
        work_root (str):
        env_ready (bool): The conversion environment matches the runtime and is ready.
        env_estimated_bytes (int): Rough size a first build of the environment adds (0 when it is ready).
        architectures_checked (bool): The architecture list of the converter was available (false: only the task was
            checked).
        architecture (None | str | Unset): `architectures[0]` of the repo's config.json.
        task (None | str | Unset):
        license_ (None | str | Unset):
        reason (None | str | Unset): Why it does not: a keyed error (`convert.unsupported_architecture`...).
        embedding_parameters (int | Unset): Parameters of the token embedding (`vocab_size * hidden_size` of the config;
            0 unknown):
            kept at a higher precision than the rest, so a small model's file is bigger than its
            parameter count says.
        fallback_parameters (int | Unset): Weights of matrices whose rows are not a multiple of 256 (0 unknown or none):
            a k-quant
            target cannot hold them, so `llama-quantize` stores them in a wider fallback type.
        runtime_tag (None | str | Unset): llama.cpp tag of the managed runtime the GGUF is made for.
        quant_method (None | str | Unset): `quantization_config.quant_method` of a pre-quantized checkpoint (`awq`,
            `fp8`...).
        base_model (None | str | Unset): The model this one was made from (`base_model` of the card): for a checkpoint
            the
            converter cannot read, a GGUF of the base model is the way to run it.
    """

    repo_id: str
    gated: bool
    convertible: bool
    vision: bool
    files: list[PlanFile]
    download_bytes: int
    parameters: int
    quants: list[QuantEstimate]
    default_quant: str
    free_bytes: int
    work_root: str
    env_ready: bool
    env_estimated_bytes: int
    architectures_checked: bool
    architecture: None | str | Unset = UNSET
    task: None | str | Unset = UNSET
    license_: None | str | Unset = UNSET
    reason: None | str | Unset = UNSET
    embedding_parameters: int | Unset = UNSET
    fallback_parameters: int | Unset = UNSET
    runtime_tag: None | str | Unset = UNSET
    quant_method: None | str | Unset = UNSET
    base_model: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        repo_id = self.repo_id

        gated = self.gated

        convertible = self.convertible

        vision = self.vision

        files = []
        for files_item_data in self.files:
            files_item = files_item_data.to_dict()
            files.append(files_item)

        download_bytes = self.download_bytes

        parameters = self.parameters

        quants = []
        for quants_item_data in self.quants:
            quants_item = quants_item_data.to_dict()
            quants.append(quants_item)

        default_quant = self.default_quant

        free_bytes = self.free_bytes

        work_root = self.work_root

        env_ready = self.env_ready

        env_estimated_bytes = self.env_estimated_bytes

        architectures_checked = self.architectures_checked

        architecture: None | str | Unset
        if isinstance(self.architecture, Unset):
            architecture = UNSET
        else:
            architecture = self.architecture

        task: None | str | Unset
        if isinstance(self.task, Unset):
            task = UNSET
        else:
            task = self.task

        license_: None | str | Unset
        if isinstance(self.license_, Unset):
            license_ = UNSET
        else:
            license_ = self.license_

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

        embedding_parameters = self.embedding_parameters

        fallback_parameters = self.fallback_parameters

        runtime_tag: None | str | Unset
        if isinstance(self.runtime_tag, Unset):
            runtime_tag = UNSET
        else:
            runtime_tag = self.runtime_tag

        quant_method: None | str | Unset
        if isinstance(self.quant_method, Unset):
            quant_method = UNSET
        else:
            quant_method = self.quant_method

        base_model: None | str | Unset
        if isinstance(self.base_model, Unset):
            base_model = UNSET
        else:
            base_model = self.base_model

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "repo_id": repo_id,
                "gated": gated,
                "convertible": convertible,
                "vision": vision,
                "files": files,
                "download_bytes": download_bytes,
                "parameters": parameters,
                "quants": quants,
                "default_quant": default_quant,
                "free_bytes": free_bytes,
                "work_root": work_root,
                "env_ready": env_ready,
                "env_estimated_bytes": env_estimated_bytes,
                "architectures_checked": architectures_checked,
            }
        )
        if architecture is not UNSET:
            field_dict["architecture"] = architecture
        if task is not UNSET:
            field_dict["task"] = task
        if license_ is not UNSET:
            field_dict["license"] = license_
        if reason is not UNSET:
            field_dict["reason"] = reason
        if embedding_parameters is not UNSET:
            field_dict["embedding_parameters"] = embedding_parameters
        if fallback_parameters is not UNSET:
            field_dict["fallback_parameters"] = fallback_parameters
        if runtime_tag is not UNSET:
            field_dict["runtime_tag"] = runtime_tag
        if quant_method is not UNSET:
            field_dict["quant_method"] = quant_method
        if base_model is not UNSET:
            field_dict["base_model"] = base_model

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.plan_file import PlanFile  # noqa: PLC0415
        from ..models.quant_estimate import QuantEstimate  # noqa: PLC0415

        d = dict(src_dict)
        repo_id = d.pop("repo_id")

        gated = d.pop("gated")

        convertible = d.pop("convertible")

        vision = d.pop("vision")

        files = []
        _files = d.pop("files")
        for files_item_data in _files:
            files_item = PlanFile.from_dict(files_item_data)

            files.append(files_item)

        download_bytes = d.pop("download_bytes")

        parameters = d.pop("parameters")

        quants = []
        _quants = d.pop("quants")
        for quants_item_data in _quants:
            quants_item = QuantEstimate.from_dict(quants_item_data)

            quants.append(quants_item)

        default_quant = d.pop("default_quant")

        free_bytes = d.pop("free_bytes")

        work_root = d.pop("work_root")

        env_ready = d.pop("env_ready")

        env_estimated_bytes = d.pop("env_estimated_bytes")

        architectures_checked = d.pop("architectures_checked")

        def _parse_architecture(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        architecture = _parse_architecture(d.pop("architecture", UNSET))

        def _parse_task(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        task = _parse_task(d.pop("task", UNSET))

        def _parse_license_(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        license_ = _parse_license_(d.pop("license", UNSET))

        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))

        embedding_parameters = d.pop("embedding_parameters", UNSET)

        fallback_parameters = d.pop("fallback_parameters", UNSET)

        def _parse_runtime_tag(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        runtime_tag = _parse_runtime_tag(d.pop("runtime_tag", UNSET))

        def _parse_quant_method(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        quant_method = _parse_quant_method(d.pop("quant_method", UNSET))

        def _parse_base_model(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        base_model = _parse_base_model(d.pop("base_model", UNSET))

        convert_plan = cls(
            repo_id=repo_id,
            gated=gated,
            convertible=convertible,
            vision=vision,
            files=files,
            download_bytes=download_bytes,
            parameters=parameters,
            quants=quants,
            default_quant=default_quant,
            free_bytes=free_bytes,
            work_root=work_root,
            env_ready=env_ready,
            env_estimated_bytes=env_estimated_bytes,
            architectures_checked=architectures_checked,
            architecture=architecture,
            task=task,
            license_=license_,
            reason=reason,
            embedding_parameters=embedding_parameters,
            fallback_parameters=fallback_parameters,
            runtime_tag=runtime_tag,
            quant_method=quant_method,
            base_model=base_model,
        )

        convert_plan.additional_properties = d
        return convert_plan

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
