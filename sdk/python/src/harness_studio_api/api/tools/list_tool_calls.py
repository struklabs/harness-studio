from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.tool_call_row import ToolCallRow
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    tool: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["tool"] = tool

    params["client"] = client_query

    params["from_ms"] = from_ms

    params["before_ms"] = before_ms

    params["limit"] = limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/tools/log",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | list[ToolCallRow] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = ToolCallRow.from_dict(response_200_item_data)

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
) -> Response[ErrorBody | list[ToolCallRow]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    tool: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> Response[ErrorBody | list[ToolCallRow]]:
    """The tool log, newest first: every call from every client (also the refused ones), with the
    arguments (long texts shortened), duration, outcome and network target. Needs `admin`.

    Args:
        tool (str | Unset):
        client_query (str | Unset):
        from_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[ToolCallRow]]
    """

    kwargs = _get_kwargs(
        tool=tool,
        client_query=client_query,
        from_ms=from_ms,
        before_ms=before_ms,
        limit=limit,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    tool: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> ErrorBody | list[ToolCallRow] | None:
    """The tool log, newest first: every call from every client (also the refused ones), with the
    arguments (long texts shortened), duration, outcome and network target. Needs `admin`.

    Args:
        tool (str | Unset):
        client_query (str | Unset):
        from_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[ToolCallRow]
    """

    return sync_detailed(
        client=client,
        tool=tool,
        client_query=client_query,
        from_ms=from_ms,
        before_ms=before_ms,
        limit=limit,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    tool: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> Response[ErrorBody | list[ToolCallRow]]:
    """The tool log, newest first: every call from every client (also the refused ones), with the
    arguments (long texts shortened), duration, outcome and network target. Needs `admin`.

    Args:
        tool (str | Unset):
        client_query (str | Unset):
        from_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[ToolCallRow]]
    """

    kwargs = _get_kwargs(
        tool=tool,
        client_query=client_query,
        from_ms=from_ms,
        before_ms=before_ms,
        limit=limit,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    tool: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> ErrorBody | list[ToolCallRow] | None:
    """The tool log, newest first: every call from every client (also the refused ones), with the
    arguments (long texts shortened), duration, outcome and network target. Needs `admin`.

    Args:
        tool (str | Unset):
        client_query (str | Unset):
        from_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[ToolCallRow]
    """

    return (
        await asyncio_detailed(
            client=client,
            tool=tool,
            client_query=client_query,
            from_ms=from_ms,
            before_ms=before_ms,
            limit=limit,
        )
    ).parsed
