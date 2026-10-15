from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.compact_body import CompactBody
from ...models.compact_response import CompactResponse
from ...models.error_body import ErrorBody
from ...types import Response


def _get_kwargs(
    id: str,
    *,
    body: CompactBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/conversations/{id}/compact".format(
            id=quote(str(id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> CompactResponse | ErrorBody | None:
    if response.status_code == 200:
        response_200 = CompactResponse.from_dict(response.json())

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

    if response.status_code == 422:
        response_422 = ErrorBody.from_dict(response.json())

        return response_422

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[CompactResponse | ErrorBody]:
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
    body: CompactBody,
) -> Response[CompactResponse | ErrorBody]:
    """Compacts the conversation's current path: the older turns are replaced, in requests, by a
    summary the model writes; the newest turns stay. Appends a marker message (nothing is
    deleted; `PUT .../compactions/{message_id}` undoes it). The summary request goes through
    the gateway (`harness serve --gateway` or the app's gateway), so it queues like any other
    request. One compaction per conversation at a time (409 `compaction.busy`).

    Args:
        id (str):
        body (CompactBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CompactResponse | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CompactBody,
) -> CompactResponse | ErrorBody | None:
    """Compacts the conversation's current path: the older turns are replaced, in requests, by a
    summary the model writes; the newest turns stay. Appends a marker message (nothing is
    deleted; `PUT .../compactions/{message_id}` undoes it). The summary request goes through
    the gateway (`harness serve --gateway` or the app's gateway), so it queues like any other
    request. One compaction per conversation at a time (409 `compaction.busy`).

    Args:
        id (str):
        body (CompactBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CompactResponse | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CompactBody,
) -> Response[CompactResponse | ErrorBody]:
    """Compacts the conversation's current path: the older turns are replaced, in requests, by a
    summary the model writes; the newest turns stay. Appends a marker message (nothing is
    deleted; `PUT .../compactions/{message_id}` undoes it). The summary request goes through
    the gateway (`harness serve --gateway` or the app's gateway), so it queues like any other
    request. One compaction per conversation at a time (409 `compaction.busy`).

    Args:
        id (str):
        body (CompactBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CompactResponse | ErrorBody]
    """

    kwargs = _get_kwargs(
        id=id,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CompactBody,
) -> CompactResponse | ErrorBody | None:
    """Compacts the conversation's current path: the older turns are replaced, in requests, by a
    summary the model writes; the newest turns stay. Appends a marker message (nothing is
    deleted; `PUT .../compactions/{message_id}` undoes it). The summary request goes through
    the gateway (`harness serve --gateway` or the app's gateway), so it queues like any other
    request. One compaction per conversation at a time (409 `compaction.busy`).

    Args:
        id (str):
        body (CompactBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CompactResponse | ErrorBody
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            body=body,
        )
    ).parsed
