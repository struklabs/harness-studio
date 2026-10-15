from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.mcp_server_status import McpServerStatus
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    retry: bool | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["retry"] = retry

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/tools/mcp",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | list[McpServerStatus] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = McpServerStatus.from_dict(response_200_item_data)

            response_200.append(response_200_item)

        return response_200

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | list[McpServerStatus]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    retry: bool | Unset = UNSET,
) -> Response[ErrorBody | list[McpServerStatus]]:
    """Harness's own MCP client: each configured server (`mcp_servers.json`) with its state
    (`idle`, `connected`, `failed` + `error`, `disabled`), transport, negotiated MCP version and
    the number of tools it registered. Servers are started on the first call.

    Args:
        retry (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[McpServerStatus]]
    """

    kwargs = _get_kwargs(
        retry=retry,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    retry: bool | Unset = UNSET,
) -> ErrorBody | list[McpServerStatus] | None:
    """Harness's own MCP client: each configured server (`mcp_servers.json`) with its state
    (`idle`, `connected`, `failed` + `error`, `disabled`), transport, negotiated MCP version and
    the number of tools it registered. Servers are started on the first call.

    Args:
        retry (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[McpServerStatus]
    """

    return sync_detailed(
        client=client,
        retry=retry,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    retry: bool | Unset = UNSET,
) -> Response[ErrorBody | list[McpServerStatus]]:
    """Harness's own MCP client: each configured server (`mcp_servers.json`) with its state
    (`idle`, `connected`, `failed` + `error`, `disabled`), transport, negotiated MCP version and
    the number of tools it registered. Servers are started on the first call.

    Args:
        retry (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[McpServerStatus]]
    """

    kwargs = _get_kwargs(
        retry=retry,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    retry: bool | Unset = UNSET,
) -> ErrorBody | list[McpServerStatus] | None:
    """Harness's own MCP client: each configured server (`mcp_servers.json`) with its state
    (`idle`, `connected`, `failed` + `error`, `disabled`), transport, negotiated MCP version and
    the number of tools it registered. Servers are started on the first call.

    Args:
        retry (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[McpServerStatus]
    """

    return (
        await asyncio_detailed(
            client=client,
            retry=retry,
        )
    ).parsed
