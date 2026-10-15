from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.release_pick import ReleasePick
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    tag: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["tag"] = tag

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/lemonade/release",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | ReleasePick | None:
    if response.status_code == 200:
        response_200 = ReleasePick.from_dict(response.json())

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
) -> Response[ErrorBody | ReleasePick]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    tag: str | Unset = UNSET,
) -> Response[ErrorBody | ReleasePick]:
    """The archive an install would download (name, size, URL, SHA-256), without downloading.

    Args:
        tag (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ReleasePick]
    """

    kwargs = _get_kwargs(
        tag=tag,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    tag: str | Unset = UNSET,
) -> ErrorBody | ReleasePick | None:
    """The archive an install would download (name, size, URL, SHA-256), without downloading.

    Args:
        tag (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ReleasePick
    """

    return sync_detailed(
        client=client,
        tag=tag,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    tag: str | Unset = UNSET,
) -> Response[ErrorBody | ReleasePick]:
    """The archive an install would download (name, size, URL, SHA-256), without downloading.

    Args:
        tag (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ReleasePick]
    """

    kwargs = _get_kwargs(
        tag=tag,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    tag: str | Unset = UNSET,
) -> ErrorBody | ReleasePick | None:
    """The archive an install would download (name, size, URL, SHA-256), without downloading.

    Args:
        tag (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ReleasePick
    """

    return (
        await asyncio_detailed(
            client=client,
            tag=tag,
        )
    ).parsed
