from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.resource_list import ResourceList
from ...types import Response


def _get_kwargs(
    server: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/tools/mcp/{server}/resources".format(
            server=quote(str(server), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | ResourceList | None:
    if response.status_code == 200:
        response_200 = ResourceList.from_dict(response.json())

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
) -> Response[ErrorBody | ResourceList]:
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
) -> Response[ErrorBody | ResourceList]:
    """The resources and resource templates of an MCP server (`resources/list`,
    `resources/templates/list`). Needs `tools`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ResourceList]
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
) -> ErrorBody | ResourceList | None:
    """The resources and resource templates of an MCP server (`resources/list`,
    `resources/templates/list`). Needs `tools`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ResourceList
    """

    return sync_detailed(
        server=server,
        client=client,
    ).parsed


async def asyncio_detailed(
    server: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | ResourceList]:
    """The resources and resource templates of an MCP server (`resources/list`,
    `resources/templates/list`). Needs `tools`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ResourceList]
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
) -> ErrorBody | ResourceList | None:
    """The resources and resource templates of an MCP server (`resources/list`,
    `resources/templates/list`). Needs `tools`.

    Args:
        server (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ResourceList
    """

    return (
        await asyncio_detailed(
            server=server,
            client=client,
        )
    ).parsed
