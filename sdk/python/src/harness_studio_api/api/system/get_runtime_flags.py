from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.runtime_flag import RuntimeFlag
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    server_id: str | Unset = UNSET,
    runtime_id: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["server_id"] = server_id

    params["runtime_id"] = runtime_id

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/runtime/flags",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | list[RuntimeFlag] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = RuntimeFlag.from_dict(response_200_item_data)

            response_200.append(response_200_item)

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
) -> Response[ErrorBody | list[RuntimeFlag]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    server_id: str | Unset = UNSET,
    runtime_id: str | Unset = UNSET,
) -> Response[ErrorBody | list[RuntimeFlag]]:
    """Every flag of a runtime's `--help`, read through the runtime's engine (its family's
    parameter catalog marks the flags a catalog parameter covers). Without parameters: the
    runtime the default server uses, else the official one.

    Args:
        server_id (str | Unset):
        runtime_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[RuntimeFlag]]
    """

    kwargs = _get_kwargs(
        server_id=server_id,
        runtime_id=runtime_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    server_id: str | Unset = UNSET,
    runtime_id: str | Unset = UNSET,
) -> ErrorBody | list[RuntimeFlag] | None:
    """Every flag of a runtime's `--help`, read through the runtime's engine (its family's
    parameter catalog marks the flags a catalog parameter covers). Without parameters: the
    runtime the default server uses, else the official one.

    Args:
        server_id (str | Unset):
        runtime_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[RuntimeFlag]
    """

    return sync_detailed(
        client=client,
        server_id=server_id,
        runtime_id=runtime_id,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    server_id: str | Unset = UNSET,
    runtime_id: str | Unset = UNSET,
) -> Response[ErrorBody | list[RuntimeFlag]]:
    """Every flag of a runtime's `--help`, read through the runtime's engine (its family's
    parameter catalog marks the flags a catalog parameter covers). Without parameters: the
    runtime the default server uses, else the official one.

    Args:
        server_id (str | Unset):
        runtime_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[RuntimeFlag]]
    """

    kwargs = _get_kwargs(
        server_id=server_id,
        runtime_id=runtime_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    server_id: str | Unset = UNSET,
    runtime_id: str | Unset = UNSET,
) -> ErrorBody | list[RuntimeFlag] | None:
    """Every flag of a runtime's `--help`, read through the runtime's engine (its family's
    parameter catalog marks the flags a catalog parameter covers). Without parameters: the
    runtime the default server uses, else the official one.

    Args:
        server_id (str | Unset):
        runtime_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[RuntimeFlag]
    """

    return (
        await asyncio_detailed(
            client=client,
            server_id=server_id,
            runtime_id=runtime_id,
        )
    ).parsed
