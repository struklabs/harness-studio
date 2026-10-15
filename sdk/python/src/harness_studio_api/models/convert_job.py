from __future__ import annotations

from collections.abc import Mapping
from typing import Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.convert_stage import ConvertStage, check_convert_stage
from ..models.convert_status import ConvertStatus, check_convert_status
from ..types import UNSET, Unset

T = TypeVar("T", bound="ConvertJob")


@_attrs_define
class ConvertJob:
    """One conversion, as the queue shows it.

    Attributes:
        id (int):
        repo_id (str):
        quant (str): Target type (`Q4_K_M`, `F16`...).
        keep_original (bool): The downloaded safetensors are kept in the library after the conversion.
        status (ConvertStatus):
        stage (ConvertStage):
        detail (str): The file or line the stage is on (technical text, not translated).
        downloaded_bytes (int): Bytes of the download stage.
        total_bytes (int):
        kept_bytes (int): Size of the partial files a failed or cancelled job left for a retry.
        created_at (int):
        updated_at (int):
        progress (float | None | Unset): Fraction of the current stage, `None` when it has no measure.
        error (None | str | Unset): Keyed error of a failed job.
        output (None | str | Unset): The finished GGUF.
        mmproj (None | str | Unset): The finished vision projector, when the model has one.
        warning (None | str | Unset): Keyed note that does not fail the job (`convert.mmproj_failed`).
    """

    id: int
    repo_id: str
    quant: str
    keep_original: bool
    status: ConvertStatus
    stage: ConvertStage
    detail: str
    downloaded_bytes: int
    total_bytes: int
    kept_bytes: int
    created_at: int
    updated_at: int
    progress: float | None | Unset = UNSET
    error: None | str | Unset = UNSET
    output: None | str | Unset = UNSET
    mmproj: None | str | Unset = UNSET
    warning: None | str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        id = self.id

        repo_id = self.repo_id

        quant = self.quant

        keep_original = self.keep_original

        status: str = self.status

        stage: str = self.stage

        detail = self.detail

        downloaded_bytes = self.downloaded_bytes

        total_bytes = self.total_bytes

        kept_bytes = self.kept_bytes

        created_at = self.created_at

        updated_at = self.updated_at

        progress: float | None | Unset
        if isinstance(self.progress, Unset):
            progress = UNSET
        else:
            progress = self.progress

        error: None | str | Unset
        if isinstance(self.error, Unset):
            error = UNSET
        else:
            error = self.error

        output: None | str | Unset
        if isinstance(self.output, Unset):
            output = UNSET
        else:
            output = self.output

        mmproj: None | str | Unset
        if isinstance(self.mmproj, Unset):
            mmproj = UNSET
        else:
            mmproj = self.mmproj

        warning: None | str | Unset
        if isinstance(self.warning, Unset):
            warning = UNSET
        else:
            warning = self.warning

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "repo_id": repo_id,
                "quant": quant,
                "keep_original": keep_original,
                "status": status,
                "stage": stage,
                "detail": detail,
                "downloaded_bytes": downloaded_bytes,
                "total_bytes": total_bytes,
                "kept_bytes": kept_bytes,
                "created_at": created_at,
                "updated_at": updated_at,
            }
        )
        if progress is not UNSET:
            field_dict["progress"] = progress
        if error is not UNSET:
            field_dict["error"] = error
        if output is not UNSET:
            field_dict["output"] = output
        if mmproj is not UNSET:
            field_dict["mmproj"] = mmproj
        if warning is not UNSET:
            field_dict["warning"] = warning

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        d = dict(src_dict)
        id = d.pop("id")

        repo_id = d.pop("repo_id")

        quant = d.pop("quant")

        keep_original = d.pop("keep_original")

        status = check_convert_status(d.pop("status"))

        stage = check_convert_stage(d.pop("stage"))

        detail = d.pop("detail")

        downloaded_bytes = d.pop("downloaded_bytes")

        total_bytes = d.pop("total_bytes")

        kept_bytes = d.pop("kept_bytes")

        created_at = d.pop("created_at")

        updated_at = d.pop("updated_at")

        def _parse_progress(data: object) -> float | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(float | None | Unset, data)

        progress = _parse_progress(d.pop("progress", UNSET))

        def _parse_error(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        error = _parse_error(d.pop("error", UNSET))

        def _parse_output(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        output = _parse_output(d.pop("output", UNSET))

        def _parse_mmproj(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        mmproj = _parse_mmproj(d.pop("mmproj", UNSET))

        def _parse_warning(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        warning = _parse_warning(d.pop("warning", UNSET))

        convert_job = cls(
            id=id,
            repo_id=repo_id,
            quant=quant,
            keep_original=keep_original,
            status=status,
            stage=stage,
            detail=detail,
            downloaded_bytes=downloaded_bytes,
            total_bytes=total_bytes,
            kept_bytes=kept_bytes,
            created_at=created_at,
            updated_at=updated_at,
            progress=progress,
            error=error,
            output=output,
            mmproj=mmproj,
            warning=warning,
        )

        convert_job.additional_properties = d
        return convert_job

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
