from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.add_runtime_request_github import AddRuntimeRequestGithub
from ...models.add_runtime_request_local import AddRuntimeRequestLocal
from ...models.error_body import ErrorBody
from ...models.runtime_entry import RuntimeEntry
from ...types import Response


def _get_kwargs(
    *,
    body: AddRuntimeRequestGithub | AddRuntimeRequestLocal,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/runtimes",
    }

    if isinstance(body, AddRuntimeRequestGithub):
        _kwargs["json"] = body.to_dict()
    else:
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | RuntimeEntry | None:
    if response.status_code == 201:
        response_201 = RuntimeEntry.from_dict(response.json())

        return response_201

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 409:
        response_409 = ErrorBody.from_dict(response.json())

        return response_409

    if response.status_code == 422:
        response_422 = ErrorBody.from_dict(response.json())

        return response_422

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | RuntimeEntry]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AddRuntimeRequestGithub | AddRuntimeRequestLocal,
) -> Response[ErrorBody | RuntimeEntry]:
    """Adds a runtime. `github`: downloads the release archive (checked against GitHub's SHA-256
    digest; an archive without one is refused with 409 `runtime.integrity_unverified` unless
    `allow_unverified`), extracts it under the data directory and runs `--version`. `local`:
    runs `--version` and hashes the folder's executables (in place, or copied with `copy`).

    Args:
        body (AddRuntimeRequestGithub | AddRuntimeRequestLocal): Add request: a GitHub release
            (`github`) or a local folder (`local`).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RuntimeEntry]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: AddRuntimeRequestGithub | AddRuntimeRequestLocal,
) -> ErrorBody | RuntimeEntry | None:
    """Adds a runtime. `github`: downloads the release archive (checked against GitHub's SHA-256
    digest; an archive without one is refused with 409 `runtime.integrity_unverified` unless
    `allow_unverified`), extracts it under the data directory and runs `--version`. `local`:
    runs `--version` and hashes the folder's executables (in place, or copied with `copy`).

    Args:
        body (AddRuntimeRequestGithub | AddRuntimeRequestLocal): Add request: a GitHub release
            (`github`) or a local folder (`local`).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RuntimeEntry
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AddRuntimeRequestGithub | AddRuntimeRequestLocal,
) -> Response[ErrorBody | RuntimeEntry]:
    """Adds a runtime. `github`: downloads the release archive (checked against GitHub's SHA-256
    digest; an archive without one is refused with 409 `runtime.integrity_unverified` unless
    `allow_unverified`), extracts it under the data directory and runs `--version`. `local`:
    runs `--version` and hashes the folder's executables (in place, or copied with `copy`).

    Args:
        body (AddRuntimeRequestGithub | AddRuntimeRequestLocal): Add request: a GitHub release
            (`github`) or a local folder (`local`).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RuntimeEntry]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: AddRuntimeRequestGithub | AddRuntimeRequestLocal,
) -> ErrorBody | RuntimeEntry | None:
    """Adds a runtime. `github`: downloads the release archive (checked against GitHub's SHA-256
    digest; an archive without one is refused with 409 `runtime.integrity_unverified` unless
    `allow_unverified`), extracts it under the data directory and runs `--version`. `local`:
    runs `--version` and hashes the folder's executables (in place, or copied with `copy`).

    Args:
        body (AddRuntimeRequestGithub | AddRuntimeRequestLocal): Add request: a GitHub release
            (`github`) or a local folder (`local`).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RuntimeEntry
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
