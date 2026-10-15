from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: int,
    *,
    delete_partial: bool | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["delete_partial"] = delete_partial

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/api/harness/v1/downloads/{id}".format(
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

    if response.status_code == 422:
        response_422 = ErrorBody.from_dict(response.json())

        return response_422

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
    id: int,
    *,
    client: AuthenticatedClient | Client,
    delete_partial: bool | Unset = UNSET,
) -> Response[Any | ErrorBody]:
    """Removes a job that is not running (pause it first).

    Args:
        id (int):
        delete_partial (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        delete_partial=delete_partial,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    delete_partial: bool | Unset = UNSET,
) -> Any | ErrorBody | None:
    """Removes a job that is not running (pause it first).

    Args:
        id (int):
        delete_partial (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
        delete_partial=delete_partial,
    ).parsed


async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    delete_partial: bool | Unset = UNSET,
) -> Response[Any | ErrorBody]:
    """Removes a job that is not running (pause it first).

    Args:
        id (int):
        delete_partial (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        delete_partial=delete_partial,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient | Client,
    delete_partial: bool | Unset = UNSET,
) -> Any | ErrorBody | None:
    """Removes a job that is not running (pause it first).

    Args:
        id (int):
        delete_partial (bool | Unset):

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
            delete_partial=delete_partial,
        )
    ).parsed
