from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: str,
    *,
    force: bool | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["force"] = force

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/api/harness/v1/runtimes/{id}".format(
            id=quote(str(id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ErrorBody | None:
    if response.status_code == 204:
        response_204 = cast(Any, None)
        return response_204

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
) -> Response[Any | ErrorBody]:
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
    force: bool | Unset = UNSET,
) -> Response[Any | ErrorBody]:
    """Unregisters a runtime; a folder Harness created (download/copy) is deleted, a local folder
    used in place is kept. 409 `runtime.in_use` while a server runs it (or, without `force`,
    while definitions/profiles choose it).

    Args:
        id (str):
        force (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        force=force,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    force: bool | Unset = UNSET,
) -> Any | ErrorBody | None:
    """Unregisters a runtime; a folder Harness created (download/copy) is deleted, a local folder
    used in place is kept. 409 `runtime.in_use` while a server runs it (or, without `force`,
    while definitions/profiles choose it).

    Args:
        id (str):
        force (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
        force=force,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    force: bool | Unset = UNSET,
) -> Response[Any | ErrorBody]:
    """Unregisters a runtime; a folder Harness created (download/copy) is deleted, a local folder
    used in place is kept. 409 `runtime.in_use` while a server runs it (or, without `force`,
    while definitions/profiles choose it).

    Args:
        id (str):
        force (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        force=force,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    force: bool | Unset = UNSET,
) -> Any | ErrorBody | None:
    """Unregisters a runtime; a folder Harness created (download/copy) is deleted, a local folder
    used in place is kept. 409 `runtime.in_use` while a server runs it (or, without `force`,
    while definitions/profiles choose it).

    Args:
        id (str):
        force (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorBody
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            force=force,
        )
    ).parsed
