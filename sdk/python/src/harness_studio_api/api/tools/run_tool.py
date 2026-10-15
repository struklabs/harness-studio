from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.run_tool_request import RunToolRequest
from ...models.run_tool_response import RunToolResponse
from ...types import Response


def _get_kwargs(
    name: str,
    *,
    body: RunToolRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/tools/{name}/run".format(
            name=quote(str(name), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | RunToolResponse | None:
    if response.status_code == 200:
        response_200 = RunToolResponse.from_dict(response.json())

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
) -> Response[ErrorBody | RunToolResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    name: str,
    *,
    client: AuthenticatedClient | Client,
    body: RunToolRequest,
) -> Response[ErrorBody | RunToolResponse]:
    """Runs one tool call. The policy decides: `403 tools.approval_required` when it asks and the
    body is not `approved`, `403 tools.disabled` when tools are off. Otherwise `200` with the
    result; `error` is set when the tool ran but failed (see the module docs).

    Args:
        name (str):
        body (RunToolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RunToolResponse]
    """

    kwargs = _get_kwargs(
        name=name,
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    name: str,
    *,
    client: AuthenticatedClient | Client,
    body: RunToolRequest,
) -> ErrorBody | RunToolResponse | None:
    """Runs one tool call. The policy decides: `403 tools.approval_required` when it asks and the
    body is not `approved`, `403 tools.disabled` when tools are off. Otherwise `200` with the
    result; `error` is set when the tool ran but failed (see the module docs).

    Args:
        name (str):
        body (RunToolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RunToolResponse
    """

    return sync_detailed(
        name=name,
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    name: str,
    *,
    client: AuthenticatedClient | Client,
    body: RunToolRequest,
) -> Response[ErrorBody | RunToolResponse]:
    """Runs one tool call. The policy decides: `403 tools.approval_required` when it asks and the
    body is not `approved`, `403 tools.disabled` when tools are off. Otherwise `200` with the
    result; `error` is set when the tool ran but failed (see the module docs).

    Args:
        name (str):
        body (RunToolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | RunToolResponse]
    """

    kwargs = _get_kwargs(
        name=name,
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    name: str,
    *,
    client: AuthenticatedClient | Client,
    body: RunToolRequest,
) -> ErrorBody | RunToolResponse | None:
    """Runs one tool call. The policy decides: `403 tools.approval_required` when it asks and the
    body is not `approved`, `403 tools.disabled` when tools are off. Otherwise `200` with the
    result; `error` is set when the tool ran but failed (see the module docs).

    Args:
        name (str):
        body (RunToolRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | RunToolResponse
    """

    return (
        await asyncio_detailed(
            name=name,
            client=client,
            body=body,
        )
    ).parsed
