"""Client of the Harness Studio Control API.

The endpoints are in the generated package ``harness_studio_api`` (one module per operation,
grouped by tag: ``harness_studio_api.api.models.list_models``); this package finds the running
instance, builds the client and turns error answers into exceptions::

    from harness_studio import connect, unwrap
    from harness_studio_api.api.system import get_status

    client = connect()
    status = unwrap(get_status.sync_detailed(client=client))
    print(status.version)
"""

from __future__ import annotations

import json
from http import HTTPStatus
import os
import sys
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any, TypeVar

from harness_studio_api import AuthenticatedClient, Client
from harness_studio_api.types import Response

__all__ = [
    "BASE_PATH",
    "DEFAULT_BASE_URL",
    "AuthenticatedClient",
    "Client",
    "Discovered",
    "HarnessError",
    "ServerSentEvent",
    "connect",
    "create_client",
    "default_data_dir",
    "discover",
    "read_server_sent_events",
    "stream_events",
    "unwrap",
]

BASE_PATH = "/api/harness/v1"
"""Prefix of every Control API path."""
DEFAULT_BASE_URL = "http://127.0.0.1:11500"
"""Where ``harness serve`` and the app listen by default."""

T = TypeVar("T")


def default_data_dir() -> Path:
    """The per-user data directory: ``HARNESS_DATA_DIR``, else the app's folder for this system."""
    if env := os.environ.get("HARNESS_DATA_DIR"):
        return Path(env)
    app = "com.harnessstudio.app"
    home = Path.home()
    if sys.platform == "win32":
        return Path(os.environ.get("APPDATA") or home / "AppData" / "Roaming") / app
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / app
    return Path(os.environ.get("XDG_DATA_HOME") or home / ".local" / "share") / app


@dataclass(frozen=True)
class Discovered:
    base_url: str
    """``http://<host>:<port>`` of the running owner's Control API."""
    token: str
    """The session token (full access)."""
    data_dir: Path


def discover(data_dir: str | os.PathLike[str] | None = None) -> Discovered:
    """Where the running instance of ``data_dir`` listens, and its session token.

    The owner of a data directory (the app or ``harness serve``) describes itself in
    ``<data dir>/instance.json``; the session token is in ``<data dir>/api-token``.
    """
    root = Path(data_dir) if data_dir is not None else default_data_dir()
    try:
        info = json.loads((root / "instance.json").read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise RuntimeError(
            f"No running Harness Studio in {root} (start the app or `harness serve`)"
        ) from None
    api = info.get("api")
    if not api:
        raise RuntimeError("The running Harness Studio has the Control API turned off")
    host = api["host"]
    # A wildcard listener is reached on loopback.
    if host in ("0.0.0.0", "::", "[::]"):
        host = "127.0.0.1"
    if ":" in host and not host.startswith("["):
        host = f"[{host}]"
    token_path = Path(info.get("token_path") or root / "api-token")
    token = token_path.read_text(encoding="utf-8").strip()
    return Discovered(base_url=f"http://{host}:{api['port']}", token=token, data_dir=root)


def create_client(
    base_url: str = DEFAULT_BASE_URL, token: str | None = None, **kwargs: Any
) -> AuthenticatedClient | Client:
    """A client for ``base_url`` (the origin, without ``/api/harness/v1``).

    With a token (the session token or a named one) it is sent as ``Authorization: Bearer``.
    Other keyword arguments go to the generated client (``timeout``, ``verify_ssl``...).
    """
    base_url = base_url.rstrip("/")
    if token:
        return AuthenticatedClient(base_url=base_url, token=token, **kwargs)
    return Client(base_url=base_url, **kwargs)


def connect(data_dir: str | os.PathLike[str] | None = None, **kwargs: Any) -> AuthenticatedClient:
    """A client for the running instance of ``data_dir`` (see :func:`discover`)."""
    found = discover(data_dir)
    return AuthenticatedClient(base_url=found.base_url, token=found.token, **kwargs)


class HarnessError(Exception):
    """An error answer of the API.

    ``code`` is stable (``api.unauthorized``, ``api.not_found``...); ``params`` fills the
    placeholders of the localized message ``message_key``.
    """

    def __init__(
        self,
        status: int,
        code: str,
        message_key: str | None = None,
        params: dict[str, Any] | None = None,
    ) -> None:
        self.status = status
        self.code = code
        self.message_key = message_key or code
        self.params = params or {}
        extra = ", ".join(f"{k}={v}" for k, v in self.params.items())
        super().__init__(f"{status} {code}" + (f" ({extra})" if extra else ""))


def unwrap(response: Response[T]) -> T:
    """The parsed body of a successful ``sync_detailed`` / ``asyncio_detailed`` call.

    Raises :class:`HarnessError` for an error answer (``{"error": {code, message_key,
    params}}``, or the HTTP status when the body is not one).
    """
    status = int(response.status_code)
    if 200 <= status < 300:
        return response.parsed  # type: ignore[return-value]
    detail: dict[str, Any] = {}
    try:
        body = json.loads(response.content)
        if isinstance(body, dict) and isinstance(body.get("error"), dict):
            detail = body["error"]
    except ValueError:
        pass
    raise HarnessError(
        status,
        detail.get("code") or f"http.{status}",
        detail.get("message_key"),
        detail.get("params"),
    )


@dataclass(frozen=True)
class ServerSentEvent:
    event: str
    data: str


def read_server_sent_events(lines: Iterator[str]) -> Iterator[ServerSentEvent]:
    """Splits the lines of a ``text/event-stream`` body into events."""
    event, data = "message", []
    for raw in lines:
        line = raw.rstrip("\r")
        if line == "":
            if data:
                yield ServerSentEvent(event, "\n".join(data))
            event, data = "message", []
        elif line.startswith("event:"):
            event = line[6:].strip()
        elif line.startswith("data:"):
            value = line[5:]
            data.append(value[1:] if value.startswith(" ") else value)


def stream_events(
    client: AuthenticatedClient | Client, types: list[str] | None = None
) -> Iterator[tuple[str, Any]]:
    """The core event bus (``GET /events``) as ``(type, event)`` pairs, as they happen.

    ``type`` is the event type (``server_status``, ``download``...) and ``event`` its JSON;
    ``lagged`` carries the number of skipped events when the client fell behind. Runs until
    the caller stops iterating.
    """
    params = {"types": ",".join(types)} if types else None
    with client.get_httpx_client().stream(
        "GET", f"{BASE_PATH}/events", params=params, timeout=None
    ) as response:
        if response.status_code >= 400:
            response.read()
            unwrap(
                Response(
                    status_code=HTTPStatus(response.status_code),
                    content=response.content,
                    headers=response.headers,
                    parsed=None,
                )
            )
        for e in read_server_sent_events(response.iter_lines()):
            yield e.event, json.loads(e.data)
