from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.conversation_summary import ConversationSummary
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    folder_id: str | Unset = UNSET,
    unfiled: bool | Unset = UNSET,
    pinned_only: bool | Unset = UNSET,
    favorites_only: bool | Unset = UNSET,
    query: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["folder_id"] = folder_id

    params["unfiled"] = unfiled

    params["pinned_only"] = pinned_only

    params["favorites_only"] = favorites_only

    params["query"] = query

    params["limit"] = limit

    params["offset"] = offset

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/conversations",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | list[ConversationSummary] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in _response_200:
            response_200_item = ConversationSummary.from_dict(response_200_item_data)

            response_200.append(response_200_item)

        return response_200

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | list[ConversationSummary]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    folder_id: str | Unset = UNSET,
    unfiled: bool | Unset = UNSET,
    pinned_only: bool | Unset = UNSET,
    favorites_only: bool | Unset = UNSET,
    query: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,
) -> Response[ErrorBody | list[ConversationSummary]]:
    """Conversations, newest first (title, folder, model, timestamps; with `query` also the
    matching text fragment).

    Args:
        folder_id (str | Unset):
        unfiled (bool | Unset):
        pinned_only (bool | Unset):
        favorites_only (bool | Unset):
        query (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[ConversationSummary]]
    """

    kwargs = _get_kwargs(
        folder_id=folder_id,
        unfiled=unfiled,
        pinned_only=pinned_only,
        favorites_only=favorites_only,
        query=query,
        limit=limit,
        offset=offset,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    folder_id: str | Unset = UNSET,
    unfiled: bool | Unset = UNSET,
    pinned_only: bool | Unset = UNSET,
    favorites_only: bool | Unset = UNSET,
    query: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,
) -> ErrorBody | list[ConversationSummary] | None:
    """Conversations, newest first (title, folder, model, timestamps; with `query` also the
    matching text fragment).

    Args:
        folder_id (str | Unset):
        unfiled (bool | Unset):
        pinned_only (bool | Unset):
        favorites_only (bool | Unset):
        query (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[ConversationSummary]
    """

    return sync_detailed(
        client=client,
        folder_id=folder_id,
        unfiled=unfiled,
        pinned_only=pinned_only,
        favorites_only=favorites_only,
        query=query,
        limit=limit,
        offset=offset,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    folder_id: str | Unset = UNSET,
    unfiled: bool | Unset = UNSET,
    pinned_only: bool | Unset = UNSET,
    favorites_only: bool | Unset = UNSET,
    query: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,
) -> Response[ErrorBody | list[ConversationSummary]]:
    """Conversations, newest first (title, folder, model, timestamps; with `query` also the
    matching text fragment).

    Args:
        folder_id (str | Unset):
        unfiled (bool | Unset):
        pinned_only (bool | Unset):
        favorites_only (bool | Unset):
        query (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | list[ConversationSummary]]
    """

    kwargs = _get_kwargs(
        folder_id=folder_id,
        unfiled=unfiled,
        pinned_only=pinned_only,
        favorites_only=favorites_only,
        query=query,
        limit=limit,
        offset=offset,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    folder_id: str | Unset = UNSET,
    unfiled: bool | Unset = UNSET,
    pinned_only: bool | Unset = UNSET,
    favorites_only: bool | Unset = UNSET,
    query: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,
) -> ErrorBody | list[ConversationSummary] | None:
    """Conversations, newest first (title, folder, model, timestamps; with `query` also the
    matching text fragment).

    Args:
        folder_id (str | Unset):
        unfiled (bool | Unset):
        pinned_only (bool | Unset):
        favorites_only (bool | Unset):
        query (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | list[ConversationSummary]
    """

    return (
        await asyncio_detailed(
            client=client,
            folder_id=folder_id,
            unfiled=unfiled,
            pinned_only=pinned_only,
            favorites_only=favorites_only,
            query=query,
            limit=limit,
            offset=offset,
        )
    ).parsed
