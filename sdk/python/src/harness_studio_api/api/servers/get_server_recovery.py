from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.recovery_plan import RecoveryPlan
from ...types import Response


def _get_kwargs(
    id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/servers/{id}/recovery".format(
            id=quote(str(id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | RecoveryPlan | None:
    if response.status_code == 200:
        response_200 = RecoveryPlan.from_dict(response.json())

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

    if response.status_code == 409:
        response_409 = ErrorBody.from_dict(response.json())

        return response_409

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | RecoveryPlan]:
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
) -> Response[ErrorBody | RecoveryPlan]:
    """The last failure of a server and, after an out-of-memory failure, the ways to retry
    (lower `-ngl`/context/`-ub`, KV cache quantization, `--fit`, MoE experts on the CPU, stop
    another server), each with concrete values. `servers.no_failure` (409) when it has none.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RecoveryPlan]
    """

    kwargs = _get_kwargs(
        id=id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | RecoveryPlan | None:
    """The last failure of a server and, after an out-of-memory failure, the ways to retry
    (lower `-ngl`/context/`-ub`, KV cache quantization, `--fit`, MoE experts on the CPU, stop
    another server), each with concrete values. `servers.no_failure` (409) when it has none.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RecoveryPlan
    """

    return sync_detailed(
        id=id,
        client=client,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | RecoveryPlan]:
    """The last failure of a server and, after an out-of-memory failure, the ways to retry
    (lower `-ngl`/context/`-ub`, KV cache quantization, `--fit`, MoE experts on the CPU, stop
    another server), each with concrete values. `servers.no_failure` (409) when it has none.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RecoveryPlan]
    """

    kwargs = _get_kwargs(
        id=id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorBody | RecoveryPlan | None:
    """The last failure of a server and, after an out-of-memory failure, the ways to retry
    (lower `-ngl`/context/`-ub`, KV cache quantization, `--fit`, MoE experts on the CPU, stop
    another server), each with concrete values. `servers.no_failure` (409) when it has none.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RecoveryPlan
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
        )
    ).parsed
