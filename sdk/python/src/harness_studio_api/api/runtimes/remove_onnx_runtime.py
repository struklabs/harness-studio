from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...types import Response


def _get_kwargs(
    variant: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "delete",
        "url": "/api/harness/v1/onnx-runtime/{variant}".format(
            variant=quote(str(variant), safe=""),
        ),
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
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | ErrorBody]:
    """Removes an installed build (it is downloaded again on the next use).

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorBody]
    """

    kwargs = _get_kwargs(
        variant=variant,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Any | ErrorBody | None:
    """Removes an installed build (it is downloaded again on the next use).

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorBody
    """

    return sync_detailed(
        variant=variant,
        client=client,
    ).parsed


async def asyncio_detailed(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | ErrorBody]:
    """Removes an installed build (it is downloaded again on the next use).

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorBody]
    """

    kwargs = _get_kwargs(
        variant=variant,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Any | ErrorBody | None:
    """Removes an installed build (it is downloaded again on the next use).

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorBody
    """

    return (
        await asyncio_detailed(
            variant=variant,
            client=client,
        )
    ).parsed
