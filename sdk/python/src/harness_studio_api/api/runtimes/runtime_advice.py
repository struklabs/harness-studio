from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.runtime_advice import RuntimeAdvice
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    model: str,
    runtime: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["model"] = model

    params["runtime"] = runtime

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/runtimes/advice",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | RuntimeAdvice | None:
    if response.status_code == 200:
        response_200 = RuntimeAdvice.from_dict(response.json())

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
) -> Response[ErrorBody | RuntimeAdvice]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    model: str,
    runtime: str | Unset = UNSET,
) -> Response[ErrorBody | RuntimeAdvice]:
    """Which runtimes can load a model (from its GGUF tensor types) and what to use: the
    suggested registered runtime, or the fork to add (e.g. PrismML for Bonsai `PQ2_0`).

    Args:
        model (str):
        runtime (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RuntimeAdvice]
    """

    kwargs = _get_kwargs(
        model=model,
        runtime=runtime,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    model: str,
    runtime: str | Unset = UNSET,
) -> ErrorBody | RuntimeAdvice | None:
    """Which runtimes can load a model (from its GGUF tensor types) and what to use: the
    suggested registered runtime, or the fork to add (e.g. PrismML for Bonsai `PQ2_0`).

    Args:
        model (str):
        runtime (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RuntimeAdvice
    """

    return sync_detailed(
        client=client,
        model=model,
        runtime=runtime,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    model: str,
    runtime: str | Unset = UNSET,
) -> Response[ErrorBody | RuntimeAdvice]:
    """Which runtimes can load a model (from its GGUF tensor types) and what to use: the
    suggested registered runtime, or the fork to add (e.g. PrismML for Bonsai `PQ2_0`).

    Args:
        model (str):
        runtime (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RuntimeAdvice]
    """

    kwargs = _get_kwargs(
        model=model,
        runtime=runtime,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    model: str,
    runtime: str | Unset = UNSET,
) -> ErrorBody | RuntimeAdvice | None:
    """Which runtimes can load a model (from its GGUF tensor types) and what to use: the
    suggested registered runtime, or the fork to add (e.g. PrismML for Bonsai `PQ2_0`).

    Args:
        model (str):
        runtime (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RuntimeAdvice
    """

    return (
        await asyncio_detailed(
            client=client,
            model=model,
            runtime=runtime,
        )
    ).parsed
