from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.onnx_verify_report import OnnxVerifyReport
from ...types import Response


def _get_kwargs(
    variant: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/api/harness/v1/onnx-runtime/{variant}/verify".format(
            variant=quote(str(variant), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | OnnxVerifyReport | None:
    if response.status_code == 200:
        response_200 = OnnxVerifyReport.from_dict(response.json())

        return response_200

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
) -> Response[ErrorBody | OnnxVerifyReport]:
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
) -> Response[ErrorBody | OnnxVerifyReport]:
    """Re-hashes the installed files against the pinned digests.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | OnnxVerifyReport]
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
) -> ErrorBody | OnnxVerifyReport | None:
    """Re-hashes the installed files against the pinned digests.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | OnnxVerifyReport
    """

    return sync_detailed(
        variant=variant,
        client=client,
    ).parsed


async def asyncio_detailed(
    variant: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorBody | OnnxVerifyReport]:
    """Re-hashes the installed files against the pinned digests.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | OnnxVerifyReport]
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
) -> ErrorBody | OnnxVerifyReport | None:
    """Re-hashes the installed files against the pinned digests.

    Args:
        variant (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | OnnxVerifyReport
    """

    return (
        await asyncio_detailed(
            variant=variant,
            client=client,
        )
    ).parsed
