from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.delete_report import DeleteReport
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: str,
    *,
    delete_definitions: bool | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["delete_definitions"] = delete_definitions

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/api/harness/v1/models/{id}".format(
            id=quote(str(id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> DeleteReport | ErrorBody | None:
    if response.status_code == 200:
        response_200 = DeleteReport.from_dict(response.json())

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
) -> Response[DeleteReport | ErrorBody]:
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
    delete_definitions: bool | Unset = UNSET,
) -> Response[DeleteReport | ErrorBody]:
    """Same rules as the library screen: refused while a server uses one of its files
    (`models.in_use`), for linked models and for files outside the library directories.

    Args:
        id (str):
        delete_definitions (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[DeleteReport | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        delete_definitions=delete_definitions,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    delete_definitions: bool | Unset = UNSET,
) -> DeleteReport | ErrorBody | None:
    """Same rules as the library screen: refused while a server uses one of its files
    (`models.in_use`), for linked models and for files outside the library directories.

    Args:
        id (str):
        delete_definitions (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        DeleteReport | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
        delete_definitions=delete_definitions,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    delete_definitions: bool | Unset = UNSET,
) -> Response[DeleteReport | ErrorBody]:
    """Same rules as the library screen: refused while a server uses one of its files
    (`models.in_use`), for linked models and for files outside the library directories.

    Args:
        id (str):
        delete_definitions (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[DeleteReport | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        delete_definitions=delete_definitions,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    delete_definitions: bool | Unset = UNSET,
) -> DeleteReport | ErrorBody | None:
    """Same rules as the library screen: refused while a server uses one of its files
    (`models.in_use`), for linked models and for files outside the library directories.

    Args:
        id (str):
        delete_definitions (bool | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        DeleteReport | ErrorBody
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            delete_definitions=delete_definitions,
        )
    ).parsed
