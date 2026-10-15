from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.managed_runtime_status import ManagedRuntimeStatus
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/runtime/managed/rollback",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | ManagedRuntimeStatus | None:
    if response.status_code == 200:
        response_200 = ManagedRuntimeStatus.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 409:
        response_409 = ErrorBody.from_dict(response.json())

        return response_409

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | ManagedRuntimeStatus]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | ManagedRuntimeStatus]:
    """ "Roll back" (Settings > Runtime): the build in the backup slot becomes the installed one;
    the replaced build takes its place. 409 `runtime.no_backup` when the slot holds no build.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ManagedRuntimeStatus]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | ManagedRuntimeStatus | None:
    """ "Roll back" (Settings > Runtime): the build in the backup slot becomes the installed one;
    the replaced build takes its place. 409 `runtime.no_backup` when the slot holds no build.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ManagedRuntimeStatus
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | ManagedRuntimeStatus]:
    """ "Roll back" (Settings > Runtime): the build in the backup slot becomes the installed one;
    the replaced build takes its place. 409 `runtime.no_backup` when the slot holds no build.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ManagedRuntimeStatus]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | ManagedRuntimeStatus | None:
    """ "Roll back" (Settings > Runtime): the build in the backup slot becomes the installed one;
    the replaced build takes its place. 409 `runtime.no_backup` when the slot holds no build.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ManagedRuntimeStatus
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
