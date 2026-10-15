from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.installed import Installed
from ...types import Response


def _get_kwargs(
    variant: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/onnx-runtime/{variant}".format(
            variant=quote(str(variant), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | Installed | None:
    if response.status_code == 201:
        response_201 = Installed.from_dict(response.json())

        return response_201

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 422:
        response_422 = ErrorBody.from_dict(response.json())

        return response_422

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | Installed]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | Installed]:
    """Downloads and installs a build (`directml`, `cuda`, `cpu`); every file is checked against
    its pinned SHA-256.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | Installed]
    """

    kwargs = _get_kwargs(
        variant=variant,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | Installed | None:
    """Downloads and installs a build (`directml`, `cuda`, `cpu`); every file is checked against
    its pinned SHA-256.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | Installed
    """

    return sync_detailed(
        variant=variant,
        client=client,
    ).parsed


async def asyncio_detailed(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | Installed]:
    """Downloads and installs a build (`directml`, `cuda`, `cpu`); every file is checked against
    its pinned SHA-256.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | Installed]
    """

    kwargs = _get_kwargs(
        variant=variant,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | Installed | None:
    """Downloads and installs a build (`directml`, `cuda`, `cpu`); every file is checked against
    its pinned SHA-256.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | Installed
    """

    return (
        await asyncio_detailed(
            variant=variant,
            client=client,
        )
    ).parsed
