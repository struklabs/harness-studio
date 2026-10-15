from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_body import ErrorBody
from ...models.fit_result import FitResult
from ...types import UNSET, Response, Unset


def _get_kwargs(
    id: str,
    *,
    context_size: int | Unset = UNSET,
    gpu_layers: int | Unset = UNSET,
    cache_type_k: str | Unset = UNSET,
    cache_type_v: str | Unset = UNSET,
    parallel_slots: int | Unset = UNSET,
    mmproj_path: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["context_size"] = context_size

    params["gpu_layers"] = gpu_layers

    params["cache_type_k"] = cache_type_k

    params["cache_type_v"] = cache_type_v

    params["parallel_slots"] = parallel_slots

    params["mmproj_path"] = mmproj_path

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/models/{id}/fit".format(
            id=quote(str(id), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorBody | FitResult | None:
    if response.status_code == 200:
        response_200 = FitResult.from_dict(response.json())

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
) -> Response[ErrorBody | FitResult]:
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
    context_size: int | Unset = UNSET,
    gpu_layers: int | Unset = UNSET,
    cache_type_k: str | Unset = UNSET,
    cache_type_v: str | Unset = UNSET,
    parallel_slots: int | Unset = UNSET,
    mmproj_path: str | Unset = UNSET,
) -> Response[ErrorBody | FitResult]:
    """Memory estimate of a local model on this machine (`harness_core::vram`, the estimate the
    GUI and the CLI use): weights, KV cache (sliding-window aware), compute buffer, projector,
    corrected with measured loads of the same file.

    Args:
        id (str):
        context_size (int | Unset):
        gpu_layers (int | Unset):
        cache_type_k (str | Unset):
        cache_type_v (str | Unset):
        parallel_slots (int | Unset):
        mmproj_path (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | FitResult]
    """

    kwargs = _get_kwargs(
        id=id,
        context_size=context_size,
        gpu_layers=gpu_layers,
        cache_type_k=cache_type_k,
        cache_type_v=cache_type_v,
        parallel_slots=parallel_slots,
        mmproj_path=mmproj_path,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    context_size: int | Unset = UNSET,
    gpu_layers: int | Unset = UNSET,
    cache_type_k: str | Unset = UNSET,
    cache_type_v: str | Unset = UNSET,
    parallel_slots: int | Unset = UNSET,
    mmproj_path: str | Unset = UNSET,
) -> ErrorBody | FitResult | None:
    """Memory estimate of a local model on this machine (`harness_core::vram`, the estimate the
    GUI and the CLI use): weights, KV cache (sliding-window aware), compute buffer, projector,
    corrected with measured loads of the same file.

    Args:
        id (str):
        context_size (int | Unset):
        gpu_layers (int | Unset):
        cache_type_k (str | Unset):
        cache_type_v (str | Unset):
        parallel_slots (int | Unset):
        mmproj_path (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | FitResult
    """

    return sync_detailed(
        id=id,
        client=client,
        context_size=context_size,
        gpu_layers=gpu_layers,
        cache_type_k=cache_type_k,
        cache_type_v=cache_type_v,
        parallel_slots=parallel_slots,
        mmproj_path=mmproj_path,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    context_size: int | Unset = UNSET,
    gpu_layers: int | Unset = UNSET,
    cache_type_k: str | Unset = UNSET,
    cache_type_v: str | Unset = UNSET,
    parallel_slots: int | Unset = UNSET,
    mmproj_path: str | Unset = UNSET,
) -> Response[ErrorBody | FitResult]:
    """Memory estimate of a local model on this machine (`harness_core::vram`, the estimate the
    GUI and the CLI use): weights, KV cache (sliding-window aware), compute buffer, projector,
    corrected with measured loads of the same file.

    Args:
        id (str):
        context_size (int | Unset):
        gpu_layers (int | Unset):
        cache_type_k (str | Unset):
        cache_type_v (str | Unset):
        parallel_slots (int | Unset):
        mmproj_path (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorBody | FitResult]
    """

    kwargs = _get_kwargs(
        id=id,
        context_size=context_size,
        gpu_layers=gpu_layers,
        cache_type_k=cache_type_k,
        cache_type_v=cache_type_v,
        parallel_slots=parallel_slots,
        mmproj_path=mmproj_path,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    id: str,
    *,
    client: AuthenticatedClient | Client,
    context_size: int | Unset = UNSET,
    gpu_layers: int | Unset = UNSET,
    cache_type_k: str | Unset = UNSET,
    cache_type_v: str | Unset = UNSET,
    parallel_slots: int | Unset = UNSET,
    mmproj_path: str | Unset = UNSET,
) -> ErrorBody | FitResult | None:
    """Memory estimate of a local model on this machine (`harness_core::vram`, the estimate the
    GUI and the CLI use): weights, KV cache (sliding-window aware), compute buffer, projector,
    corrected with measured loads of the same file.

    Args:
        id (str):
        context_size (int | Unset):
        gpu_layers (int | Unset):
        cache_type_k (str | Unset):
        cache_type_v (str | Unset):
        parallel_slots (int | Unset):
        mmproj_path (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorBody | FitResult
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
            context_size=context_size,
            gpu_layers=gpu_layers,
            cache_type_k=cache_type_k,
            cache_type_v=cache_type_v,
            parallel_slots=parallel_slots,
            mmproj_path=mmproj_path,
        )
    ).parsed
