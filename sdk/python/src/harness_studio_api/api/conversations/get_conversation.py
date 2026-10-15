from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.conversation_view import ConversationView
from ...models.error_body import ErrorBody
from ...types import Response


def _get_kwargs(
    id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/conversations/{id}".format(
            id=quote(str(id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ConversationView | ErrorBody | None:
    if response.status_code == 200:
        response_200 = ConversationView.from_dict(response.json())

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
) -> Response[ConversationView | ErrorBody]:
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
) -> Response[ConversationView | ErrorBody]:
    """One conversation: its settings and the messages of the current path (root to leaf), each
    with the shown variant (content, reasoning, tool calls, stats). A compaction marker is a
    `system` message whose variant `meta.compaction` describes it.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConversationView | ErrorBody]
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
) -> ConversationView | ErrorBody | None:
    """One conversation: its settings and the messages of the current path (root to leaf), each
    with the shown variant (content, reasoning, tool calls, stats). A compaction marker is a
    `system` message whose variant `meta.compaction` describes it.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConversationView | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ConversationView | ErrorBody]:
    """One conversation: its settings and the messages of the current path (root to leaf), each
    with the shown variant (content, reasoning, tool calls, stats). A compaction marker is a
    `system` message whose variant `meta.compaction` describes it.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConversationView | ErrorBody]
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
) -> ConversationView | ErrorBody | None:
    """One conversation: its settings and the messages of the current path (root to leaf), each
    with the shown variant (content, reasoning, tool calls, stats). A compaction marker is a
    `system` message whose variant `meta.compaction` describes it.

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConversationView | ErrorBody
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
        )
    ).parsed
