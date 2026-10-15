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
    family: str | Unset = UNSET,
    repo: str,
    tag: str | Unset = UNSET,
    asset: str | Unset = UNSET,
    backend: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["family"] = family

    params["repo"] = repo

    params["tag"] = tag

    params["asset"] = asset

    params["backend"] = backend

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/runtimes/release",
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
    family: str | Unset = UNSET,
    repo: str,
    tag: str | Unset = UNSET,
    asset: str | Unset = UNSET,
    backend: str | Unset = UNSET,
) -> Response[ErrorBody | ReleasePick]:
    """The archive(s) a GitHub install would download (name, size, URL, digest), without
    downloading anything.

    Args:
        family (str | Unset):
        repo (str):
        tag (str | Unset):
        asset (str | Unset):
        backend (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ReleasePick]
    """

    kwargs = _get_kwargs(
        family=family,
        repo=repo,
        tag=tag,
        asset=asset,
        backend=backend,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    family: str | Unset = UNSET,
    repo: str,
    tag: str | Unset = UNSET,
    asset: str | Unset = UNSET,
    backend: str | Unset = UNSET,
) -> ErrorBody | ReleasePick | None:
    """The archive(s) a GitHub install would download (name, size, URL, digest), without
    downloading anything.

    Args:
        family (str | Unset):
        repo (str):
        tag (str | Unset):
        asset (str | Unset):
        backend (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ReleasePick
    """

    return sync_detailed(
        client=client,
        family=family,
        repo=repo,
        tag=tag,
        asset=asset,
        backend=backend,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    family: str | Unset = UNSET,
    repo: str,
    tag: str | Unset = UNSET,
    asset: str | Unset = UNSET,
    backend: str | Unset = UNSET,
) -> Response[ErrorBody | ReleasePick]:
    """The archive(s) a GitHub install would download (name, size, URL, digest), without
    downloading anything.

    Args:
        family (str | Unset):
        repo (str):
        tag (str | Unset):
        asset (str | Unset):
        backend (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ReleasePick]
    """

    kwargs = _get_kwargs(
        family=family,
        repo=repo,
        tag=tag,
        asset=asset,
        backend=backend,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    family: str | Unset = UNSET,
    repo: str,
    tag: str | Unset = UNSET,
    asset: str | Unset = UNSET,
    backend: str | Unset = UNSET,
) -> ErrorBody | ReleasePick | None:
    """The archive(s) a GitHub install would download (name, size, URL, digest), without
    downloading anything.

    Args:
        family (str | Unset):
        repo (str):
        tag (str | Unset):
        asset (str | Unset):
        backend (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ReleasePick
    """

    return (
        await asyncio_detailed(
            client=client,
            family=family,
            repo=repo,
            tag=tag,
            asset=asset,
            backend=backend,
        )
    ).parsed
