from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.modelfile_imported import ModelfileImported
from ...models.modelfile_request import ModelfileRequest
from ...types import Response


def _get_kwargs(
    *,
    body: ModelfileRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/modelfile/import",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | ModelfileImported | None:
    if response.status_code == 201:
        response_201 = ModelfileImported.from_dict(response.json())

        return response_201

    if response.status_code == 401:
        response_401 = ErrorBody.from_dict(response.json())

        return response_401

    if response.status_code == 403:
        response_403 = ErrorBody.from_dict(response.json())

        return response_403

    if response.status_code == 422:
        response_422 = ErrorBody.from_dict(response.json())

        return response_422

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorBody | ModelfileImported]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ModelfileRequest,
) -> Response[ErrorBody | ModelfileImported]:
    """Imports a Modelfile: creates the model definition (alias from the request, else derived
    from `FROM`) and its inference preset. Returns the saved definition and the warnings.

    Args:
        body (ModelfileRequest): Request of the Control API / GUI: a Modelfile's text and where
            its relative paths start.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ModelfileImported]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: ModelfileRequest,
) -> ErrorBody | ModelfileImported | None:
    """Imports a Modelfile: creates the model definition (alias from the request, else derived
    from `FROM`) and its inference preset. Returns the saved definition and the warnings.

    Args:
        body (ModelfileRequest): Request of the Control API / GUI: a Modelfile's text and where
            its relative paths start.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ModelfileImported
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ModelfileRequest,
) -> Response[ErrorBody | ModelfileImported]:
    """Imports a Modelfile: creates the model definition (alias from the request, else derived
    from `FROM`) and its inference preset. Returns the saved definition and the warnings.

    Args:
        body (ModelfileRequest): Request of the Control API / GUI: a Modelfile's text and where
            its relative paths start.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | ModelfileImported]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: ModelfileRequest,
) -> ErrorBody | ModelfileImported | None:
    """Imports a Modelfile: creates the model definition (alias from the request, else derived
    from `FROM`) and its inference preset. Returns the saved definition and the warnings.

    Args:
        body (ModelfileRequest): Request of the Control API / GUI: a Modelfile's text and where
            its relative paths start.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | ModelfileImported
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
