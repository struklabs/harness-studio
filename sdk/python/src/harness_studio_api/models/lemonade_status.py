from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.backend_info import BackendInfo
    from ..models.install_view import InstallView
    from ..models.lemonade_settings import LemonadeSettings
    from ..models.lemonade_status_install_progress_type_0 import (
        LemonadeStatusInstallProgressType0,
    )
    from ..models.npu_info import NpuInfo
    from ..models.pull_progress import PullProgress


T = TypeVar("T", bound="LemonadeStatus")


@_attrs_define
class LemonadeStatus:
    """Everything the Settings card shows.

    Attributes:
        supported (bool): Lemonade publishes a build for this OS and CPU.
        installs (list[InstallView]): Installed builds, newest first.
        settings (LemonadeSettings): Harness's settings of the Lemonade engine (`<data>/lemonade/settings.json`).
        running (bool):
        npu (NpuInfo): What the NPU detection found.
        backends (list[BackendInfo]): Backends as of the last run.
        models_dir (str):
        log_file (str):
        port (int | None | Unset):
        websocket_port (int | None | Unset):
        pid (int | None | Unset):
        base_url (None | str | Unset): `http://127.0.0.1:<port>` (the key is never shown).
        running_runtime (None | str | Unset): Registry id of the build that is running.
        running_version (None | str | Unset): Version of the running `lemond`.
        started_at (int | None | Unset):
        install_progress (LemonadeStatusInstallProgressType0 | None | Unset): Progress of the running release download.
        pull (None | PullProgress | Unset):
    """

    supported: bool
    installs: list[InstallView]
    settings: LemonadeSettings
    running: bool
    npu: NpuInfo
    backends: list[BackendInfo]
    models_dir: str
    log_file: str
    port: int | None | Unset = UNSET
    websocket_port: int | None | Unset = UNSET
    pid: int | None | Unset = UNSET
    base_url: None | str | Unset = UNSET
    running_runtime: None | str | Unset = UNSET
    running_version: None | str | Unset = UNSET
    started_at: int | None | Unset = UNSET
    install_progress: LemonadeStatusInstallProgressType0 | None | Unset = UNSET
    pull: None | PullProgress | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.lemonade_status_install_progress_type_0 import (
            LemonadeStatusInstallProgressType0,  # noqa: PLC0415
        )
        from ..models.pull_progress import PullProgress  # noqa: PLC0415

        supported = self.supported

        installs = []
        for installs_item_data in self.installs:
            installs_item = installs_item_data.to_dict()
            installs.append(installs_item)

        settings = self.settings.to_dict()

        running = self.running

        npu = self.npu.to_dict()

        backends = []
        for backends_item_data in self.backends:
            backends_item = backends_item_data.to_dict()
            backends.append(backends_item)

        models_dir = self.models_dir

        log_file = self.log_file

        port: int | None | Unset
        if isinstance(self.port, Unset):
            port = UNSET
        else:
            port = self.port

        websocket_port: int | None | Unset
        if isinstance(self.websocket_port, Unset):
            websocket_port = UNSET
        else:
            websocket_port = self.websocket_port

        pid: int | None | Unset
        if isinstance(self.pid, Unset):
            pid = UNSET
        else:
            pid = self.pid

        base_url: None | str | Unset
        if isinstance(self.base_url, Unset):
            base_url = UNSET
        else:
            base_url = self.base_url

        running_runtime: None | str | Unset
        if isinstance(self.running_runtime, Unset):
            running_runtime = UNSET
        else:
            running_runtime = self.running_runtime

        running_version: None | str | Unset
        if isinstance(self.running_version, Unset):
            running_version = UNSET
        else:
            running_version = self.running_version

        started_at: int | None | Unset
        if isinstance(self.started_at, Unset):
            started_at = UNSET
        else:
            started_at = self.started_at

        install_progress: dict[str, Any] | None | Unset
        if isinstance(self.install_progress, Unset):
            install_progress = UNSET
        elif isinstance(self.install_progress, LemonadeStatusInstallProgressType0):
            install_progress = self.install_progress.to_dict()
        else:
            install_progress = self.install_progress

        pull: dict[str, Any] | None | Unset
        if isinstance(self.pull, Unset):
            pull = UNSET
        elif isinstance(self.pull, PullProgress):
            pull = self.pull.to_dict()
        else:
            pull = self.pull

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "supported": supported,
                "installs": installs,
                "settings": settings,
                "running": running,
                "npu": npu,
                "backends": backends,
                "models_dir": models_dir,
                "log_file": log_file,
            }
        )
        if port is not UNSET:
            field_dict["port"] = port
        if websocket_port is not UNSET:
            field_dict["websocket_port"] = websocket_port
        if pid is not UNSET:
            field_dict["pid"] = pid
        if base_url is not UNSET:
            field_dict["base_url"] = base_url
        if running_runtime is not UNSET:
            field_dict["running_runtime"] = running_runtime
        if running_version is not UNSET:
            field_dict["running_version"] = running_version
        if started_at is not UNSET:
            field_dict["started_at"] = started_at
        if install_progress is not UNSET:
            field_dict["install_progress"] = install_progress
        if pull is not UNSET:
            field_dict["pull"] = pull

        return field_dict

    @classmethod
    def from_dict(cls: type[T], src_dict: Mapping[str, Any]) -> T:
        from ..models.backend_info import BackendInfo  # noqa: PLC0415
        from ..models.install_view import InstallView  # noqa: PLC0415
        from ..models.lemonade_settings import LemonadeSettings  # noqa: PLC0415
        from ..models.lemonade_status_install_progress_type_0 import (
            LemonadeStatusInstallProgressType0,  # noqa: PLC0415
        )
        from ..models.npu_info import NpuInfo  # noqa: PLC0415
        from ..models.pull_progress import PullProgress  # noqa: PLC0415

        d = dict(src_dict)
        supported = d.pop("supported")

        installs = []
        _installs = d.pop("installs")
        for installs_item_data in _installs:
            installs_item = InstallView.from_dict(installs_item_data)

            installs.append(installs_item)

        settings = LemonadeSettings.from_dict(d.pop("settings"))

        running = d.pop("running")

        npu = NpuInfo.from_dict(d.pop("npu"))

        backends = []
        _backends = d.pop("backends")
        for backends_item_data in _backends:
            backends_item = BackendInfo.from_dict(backends_item_data)

            backends.append(backends_item)

        models_dir = d.pop("models_dir")

        log_file = d.pop("log_file")

        def _parse_port(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        port = _parse_port(d.pop("port", UNSET))

        def _parse_websocket_port(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        websocket_port = _parse_websocket_port(d.pop("websocket_port", UNSET))

        def _parse_pid(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        pid = _parse_pid(d.pop("pid", UNSET))

        def _parse_base_url(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        base_url = _parse_base_url(d.pop("base_url", UNSET))

        def _parse_running_runtime(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        running_runtime = _parse_running_runtime(d.pop("running_runtime", UNSET))

        def _parse_running_version(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        running_version = _parse_running_version(d.pop("running_version", UNSET))

        def _parse_started_at(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        started_at = _parse_started_at(d.pop("started_at", UNSET))

        def _parse_install_progress(
            data: object,
        ) -> LemonadeStatusInstallProgressType0 | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                install_progress_type_0 = LemonadeStatusInstallProgressType0.from_dict(data)

                return install_progress_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(LemonadeStatusInstallProgressType0 | None | Unset, data)

        install_progress = _parse_install_progress(d.pop("install_progress", UNSET))

        def _parse_pull(data: object) -> None | PullProgress | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                pull_type_1 = PullProgress.from_dict(data)

                return pull_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | PullProgress | Unset, data)

        pull = _parse_pull(d.pop("pull", UNSET))

        lemonade_status = cls(
            supported=supported,
            installs=installs,
            settings=settings,
            running=running,
            npu=npu,
            backends=backends,
            models_dir=models_dir,
            log_file=log_file,
            port=port,
            websocket_port=websocket_port,
            pid=pid,
            base_url=base_url,
            running_runtime=running_runtime,
            running_version=running_version,
            started_at=started_at,
            install_progress=install_progress,
            pull=pull,
        )

        lemonade_status.additional_properties = d
        return lemonade_status

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
