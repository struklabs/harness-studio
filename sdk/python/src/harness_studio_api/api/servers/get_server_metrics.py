from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.metrics_response import MetricsResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: str,
    *,
    history: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["history"] = history

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/servers/{id}/metrics".format(
            id=quote(str(id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | MetricsResponse | None:
    if response.status_code == 200:
        response_200 = MetricsResponse.from_dict(response.json())

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
) -> Response[ErrorBody | MetricsResponse]:
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
    history: int | Unset = UNSET,
) -> Response[ErrorBody | MetricsResponse]:
    """Live telemetry (sampled once a second from `/metrics` and `/slots`).

    Args:
        id (str):
        history (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | MetricsResponse]
    """

    kwargs = _get_kwargs(
        id=id,
        history=history,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    history: int | Unset = UNSET,
) -> ErrorBody | MetricsResponse | None:
    """Live telemetry (sampled once a second from `/metrics` and `/slots`).

    Args:
        id (str):
        history (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | MetricsResponse
    """

    return sync_detailed(
        id=id,
        client=client,
        history=history,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    history: int | Unset = UNSET,
) -> Response[ErrorBody | MetricsResponse]:
    """Live telemetry (sampled once a second from `/metrics` and `/slots`).

    Args:
        id (str):
        history (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | MetricsResponse]
    """

    kwargs = _get_kwargs(
        id=id,
        history=history,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    history: int | Unset = UNSET,
) -> ErrorBody | MetricsResponse | None:
    """Live telemetry (sampled once a second from `/metrics` and `/slots`).

    Args:
        id (str):
        history (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | MetricsResponse
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            history=history,
        )
    ).parsed
