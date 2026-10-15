from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.request_page import RequestPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    model: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    endpoint: str | Unset = UNSET,
    status: str | Unset = UNSET,
    tool_issue: bool | Unset = UNSET,
    tool_category: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["model"] = model

    params["client"] = client_query

    params["endpoint"] = endpoint

    params["status"] = status

    params["tool_issue"] = tool_issue

    params["tool_category"] = tool_category

    params["from_ms"] = from_ms

    params["to_ms"] = to_ms

    params["before_ms"] = before_ms

    params["limit"] = limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/analytics/requests",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | RequestPage | None:
    if response.status_code == 200:
        response_200 = RequestPage.from_dict(response.json())

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
) -> Response[ErrorBody | RequestPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    model: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    endpoint: str | Unset = UNSET,
    status: str | Unset = UNSET,
    tool_issue: bool | Unset = UNSET,
    tool_category: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> Response[ErrorBody | RequestPage]:
    """Recent gateway requests, newest first (metadata only). Page with `before_ms`.

    Args:
        model (str | Unset):
        client_query (str | Unset):
        endpoint (str | Unset):
        status (str | Unset):
        tool_issue (bool | Unset):
        tool_category (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RequestPage]
    """

    kwargs = _get_kwargs(
        model=model,
        client_query=client_query,
        endpoint=endpoint,
        status=status,
        tool_issue=tool_issue,
        tool_category=tool_category,
        from_ms=from_ms,
        to_ms=to_ms,
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
    model: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    endpoint: str | Unset = UNSET,
    status: str | Unset = UNSET,
    tool_issue: bool | Unset = UNSET,
    tool_category: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> ErrorBody | RequestPage | None:
    """Recent gateway requests, newest first (metadata only). Page with `before_ms`.

    Args:
        model (str | Unset):
        client_query (str | Unset):
        endpoint (str | Unset):
        status (str | Unset):
        tool_issue (bool | Unset):
        tool_category (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RequestPage
    """

    return sync_detailed(
        client=client,
        model=model,
        client_query=client_query,
        endpoint=endpoint,
        status=status,
        tool_issue=tool_issue,
        tool_category=tool_category,
        from_ms=from_ms,
        to_ms=to_ms,
        before_ms=before_ms,
        limit=limit,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    model: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    endpoint: str | Unset = UNSET,
    status: str | Unset = UNSET,
    tool_issue: bool | Unset = UNSET,
    tool_category: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> Response[ErrorBody | RequestPage]:
    """Recent gateway requests, newest first (metadata only). Page with `before_ms`.

    Args:
        model (str | Unset):
        client_query (str | Unset):
        endpoint (str | Unset):
        status (str | Unset):
        tool_issue (bool | Unset):
        tool_category (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RequestPage]
    """

    kwargs = _get_kwargs(
        model=model,
        client_query=client_query,
        endpoint=endpoint,
        status=status,
        tool_issue=tool_issue,
        tool_category=tool_category,
        from_ms=from_ms,
        to_ms=to_ms,
        before_ms=before_ms,
        limit=limit,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    model: str | Unset = UNSET,
    client_query: str | Unset = UNSET,
    endpoint: str | Unset = UNSET,
    status: str | Unset = UNSET,
    tool_issue: bool | Unset = UNSET,
    tool_category: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
    before_ms: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> ErrorBody | RequestPage | None:
    """Recent gateway requests, newest first (metadata only). Page with `before_ms`.

    Args:
        model (str | Unset):
        client_query (str | Unset):
        endpoint (str | Unset):
        status (str | Unset):
        tool_issue (bool | Unset):
        tool_category (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):
        before_ms (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RequestPage
    """

    return (
        await asyncio_detailed(
            client=client,
            model=model,
            client_query=client_query,
            endpoint=endpoint,
            status=status,
            tool_issue=tool_issue,
            tool_category=tool_category,
            from_ms=from_ms,
            to_ms=to_ms,
            before_ms=before_ms,
            limit=limit,
        )
    ).parsed
