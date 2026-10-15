from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.logs_response import LogsResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: str,
    *,
    since: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["since"] = since

    params["limit"] = limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/servers/{id}/logs".format(
            id=quote(str(id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | LogsResponse | None:
    if response.status_code == 200:
        response_200 = LogsResponse.from_dict(response.json())

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

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | LogsResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    since: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> Response[ErrorBody | LogsResponse]:
    """Live log of the current run (ring buffer of the last 1000 lines).

    Args:
        id (str):
        since (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | LogsResponse]
    """

    kwargs = _get_kwargs(
        id=id,
        since=since,
        limit=limit,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    since: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> ErrorBody | LogsResponse | None:
    """Live log of the current run (ring buffer of the last 1000 lines).

    Args:
        id (str):
        since (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | LogsResponse
    """

    return sync_detailed(
        id=id,
        client=client,
        since=since,
        limit=limit,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    since: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> Response[ErrorBody | LogsResponse]:
    """Live log of the current run (ring buffer of the last 1000 lines).

    Args:
        id (str):
        since (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | LogsResponse]
    """

    kwargs = _get_kwargs(
        id=id,
        since=since,
        limit=limit,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    since: int | Unset = UNSET,
    limit: int | Unset = UNSET,
) -> ErrorBody | LogsResponse | None:
    """Live log of the current run (ring buffer of the last 1000 lines).

    Args:
        id (str):
        since (int | Unset):
        limit (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | LogsResponse
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            since=since,
            limit=limit,
        )
    ).parsed
