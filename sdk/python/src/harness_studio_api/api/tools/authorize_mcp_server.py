from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.mcp_server_status import McpServerStatus
from ...types import Response


def _get_kwargs(
    server: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/tools/mcp/{server}/authorize".format(
            server=quote(str(server), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | McpServerStatus | None:
    if response.status_code == 200:
        response_200 = McpServerStatus.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorBody.from_dict(response.json())

        return response_404

    if response.status_code == 422:
        response_422 = ErrorBody.from_dict(response.json())

        return response_422

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | McpServerStatus]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    server: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | McpServerStatus]:
    """Authorizes a remote MCP server (OAuth, MCP authorization spec): opens the system browser of
    the machine Harness runs on and waits up to five minutes for the user to approve. The new
    token is stored in the OS keychain and the server is connected with it. Needs `admin`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | McpServerStatus]
    """

    kwargs = _get_kwargs(
        server=server,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    server: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | McpServerStatus | None:
    """Authorizes a remote MCP server (OAuth, MCP authorization spec): opens the system browser of
    the machine Harness runs on and waits up to five minutes for the user to approve. The new
    token is stored in the OS keychain and the server is connected with it. Needs `admin`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | McpServerStatus
    """

    return sync_detailed(
        server=server,
        client=client,
    ).parsed


async def asyncio_detailed(
    server: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | McpServerStatus]:
    """Authorizes a remote MCP server (OAuth, MCP authorization spec): opens the system browser of
    the machine Harness runs on and waits up to five minutes for the user to approve. The new
    token is stored in the OS keychain and the server is connected with it. Needs `admin`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | McpServerStatus]
    """

    kwargs = _get_kwargs(
        server=server,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    server: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | McpServerStatus | None:
    """Authorizes a remote MCP server (OAuth, MCP authorization spec): opens the system browser of
    the machine Harness runs on and waits up to five minutes for the user to approve. The new
    token is stored in the OS keychain and the server is connected with it. Needs `admin`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | McpServerStatus
    """

    return (
        await asyncio_detailed(
            server=server,
            client=client,
        )
    ).parsed
