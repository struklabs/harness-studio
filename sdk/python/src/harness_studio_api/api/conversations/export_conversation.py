from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.conversation_export import ConversationExport
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: str,
    *,
    format_: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["format"] = format_

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/conversations/{id}/export".format(
            id=quote(str(id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ConversationExport | ErrorBody | None:
    if response.status_code == 200:
        response_200 = ConversationExport.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ErrorBody.from_dict(response.json())

        return response_400

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
) -> Response[ConversationExport | ErrorBody]:
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
    format_: str | Unset = UNSET,
) -> Response[ConversationExport | ErrorBody]:
    """Exports a conversation as JSON (`harness.conversation.v1`) or Markdown.

    Args:
        id (str):
        format_ (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConversationExport | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        format_=format_,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    format_: str | Unset = UNSET,
) -> ConversationExport | ErrorBody | None:
    """Exports a conversation as JSON (`harness.conversation.v1`) or Markdown.

    Args:
        id (str):
        format_ (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConversationExport | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
        format_=format_,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    format_: str | Unset = UNSET,
) -> Response[ConversationExport | ErrorBody]:
    """Exports a conversation as JSON (`harness.conversation.v1`) or Markdown.

    Args:
        id (str):
        format_ (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConversationExport | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        format_=format_,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    format_: str | Unset = UNSET,
) -> ConversationExport | ErrorBody | None:
    """Exports a conversation as JSON (`harness.conversation.v1`) or Markdown.

    Args:
        id (str):
        format_ (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConversationExport | ErrorBody
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            format_=format_,
        )
    ).parsed
