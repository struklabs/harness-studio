from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.hardware_devices_response import HardwareDevicesResponse
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    refresh: bool | Unset = UNSET,
    definition_id: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["refresh"] = refresh

    params["definition_id"] = definition_id

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/hardware/devices",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | HardwareDevicesResponse | None:
    if response.status_code == 200:
        response_200 = HardwareDevicesResponse.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 409:
        response_409 = ErrorBody.from_dict(response.json())

        return response_409

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | HardwareDevicesResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    refresh: bool | Unset = UNSET,
    definition_id: str | Unset = UNSET,
) -> Response[ErrorBody | HardwareDevicesResponse]:
    """GPU devices as the active runtime reports them (`llama-server --list-devices`: backend,
    total and free memory; free corrected with the OS usage), plus a main-GPU / tensor-split
    suggestion when there are several GPUs.

    Args:
        refresh (bool | Unset):
        definition_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | HardwareDevicesResponse]
    """

    kwargs = _get_kwargs(
        refresh=refresh,
        definition_id=definition_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    refresh: bool | Unset = UNSET,
    definition_id: str | Unset = UNSET,
) -> ErrorBody | HardwareDevicesResponse | None:
    """GPU devices as the active runtime reports them (`llama-server --list-devices`: backend,
    total and free memory; free corrected with the OS usage), plus a main-GPU / tensor-split
    suggestion when there are several GPUs.

    Args:
        refresh (bool | Unset):
        definition_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | HardwareDevicesResponse
    """

    return sync_detailed(
        client=client,
        refresh=refresh,
        definition_id=definition_id,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    refresh: bool | Unset = UNSET,
    definition_id: str | Unset = UNSET,
) -> Response[ErrorBody | HardwareDevicesResponse]:
    """GPU devices as the active runtime reports them (`llama-server --list-devices`: backend,
    total and free memory; free corrected with the OS usage), plus a main-GPU / tensor-split
    suggestion when there are several GPUs.

    Args:
        refresh (bool | Unset):
        definition_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | HardwareDevicesResponse]
    """

    kwargs = _get_kwargs(
        refresh=refresh,
        definition_id=definition_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    refresh: bool | Unset = UNSET,
    definition_id: str | Unset = UNSET,
) -> ErrorBody | HardwareDevicesResponse | None:
    """GPU devices as the active runtime reports them (`llama-server --list-devices`: backend,
    total and free memory; free corrected with the OS usage), plus a main-GPU / tensor-split
    suggestion when there are several GPUs.

    Args:
        refresh (bool | Unset):
        definition_id (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | HardwareDevicesResponse
    """

    return (
        await asyncio_detailed(
            client=client,
            refresh=refresh,
            definition_id=definition_id,
        )
    ).parsed
