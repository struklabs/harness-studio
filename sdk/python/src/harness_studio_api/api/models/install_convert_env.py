from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.env_status import EnvStatus
from ...models.error_body import ErrorBody
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/convert/env",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> EnvStatus | ErrorBody | None:
    if response.status_code == 201:
        response_201 = EnvStatus.from_dict(response.json())

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
) -> Response[EnvStatus | ErrorBody]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[EnvStatus | ErrorBody]:
    """Builds the conversion environment for the managed runtime's llama.cpp tag (uv, Python and
    the converter's packages, about 700 MB). Returns when it is complete.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EnvStatus | ErrorBody]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> EnvStatus | ErrorBody | None:
    """Builds the conversion environment for the managed runtime's llama.cpp tag (uv, Python and
    the converter's packages, about 700 MB). Returns when it is complete.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EnvStatus | ErrorBody
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[EnvStatus | ErrorBody]:
    """Builds the conversion environment for the managed runtime's llama.cpp tag (uv, Python and
    the converter's packages, about 700 MB). Returns when it is complete.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EnvStatus | ErrorBody]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> EnvStatus | ErrorBody | None:
    """Builds the conversion environment for the managed runtime's llama.cpp tag (uv, Python and
    the converter's packages, about 700 MB). Returns when it is complete.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EnvStatus | ErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
