from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.install_request import InstallRequest
from ...models.runtime_entry import RuntimeEntry
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: InstallRequest | None | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/lemonade/install",
    }

    if isinstance(body, InstallRequest):
        _kwargs["json"] = body.to_dict()
    else:
        _kwargs["json"] = body

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
    body: InstallRequest | None | Unset = UNSET,
) -> Response[ErrorBody | RuntimeEntry]:
    """Downloads the Lemonade release (checked against GitHub's SHA-256), extracts it and
    registers it as a runtime of the `lemonade` family. A newer release is a new entry; the
    older one stays as the rollback.

    Args:
        body (InstallRequest | None | Unset):

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
    body: InstallRequest | None | Unset = UNSET,
) -> ErrorBody | RuntimeEntry | None:
    """Downloads the Lemonade release (checked against GitHub's SHA-256), extracts it and
    registers it as a runtime of the `lemonade` family. A newer release is a new entry; the
    older one stays as the rollback.

    Args:
        body (InstallRequest | None | Unset):

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
    body: InstallRequest | None | Unset = UNSET,
) -> Response[ErrorBody | RuntimeEntry]:
    """Downloads the Lemonade release (checked against GitHub's SHA-256), extracts it and
    registers it as a runtime of the `lemonade` family. A newer release is a new entry; the
    older one stays as the rollback.

    Args:
        body (InstallRequest | None | Unset):

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
    body: InstallRequest | None | Unset = UNSET,
) -> ErrorBody | RuntimeEntry | None:
    """Downloads the Lemonade release (checked against GitHub's SHA-256), extracts it and
    registers it as a runtime of the `lemonade` family. A newer release is a new entry; the
    older one stays as the rollback.

    Args:
        body (InstallRequest | None | Unset):

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
