from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.analytics_summary import AnalyticsSummary
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    range_: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["range"] = range_

    params["from_ms"] = from_ms

    params["to_ms"] = to_ms

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/analytics/summary",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AnalyticsSummary | ErrorBody | None:
    if response.status_code == 200:
        response_200 = AnalyticsSummary.from_dict(response.json())

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

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[AnalyticsSummary | ErrorBody]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    range_: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
) -> Response[AnalyticsSummary | ErrorBody]:
    """Per-model statistics (runs, tokens, average t/s, P50/P95 TTFT and latency, VRAM,
    error and OOM rates, busy time, last use), client distribution, daily series and the
    latest benchmark of each model. Covers gateway traffic only.

    Args:
        range_ (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AnalyticsSummary | ErrorBody]
    """

    kwargs = _get_kwargs(
        range_=range_,
        from_ms=from_ms,
        to_ms=to_ms,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    range_: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
) -> AnalyticsSummary | ErrorBody | None:
    """Per-model statistics (runs, tokens, average t/s, P50/P95 TTFT and latency, VRAM,
    error and OOM rates, busy time, last use), client distribution, daily series and the
    latest benchmark of each model. Covers gateway traffic only.

    Args:
        range_ (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AnalyticsSummary | ErrorBody
    """

    return sync_detailed(
        client=client,
        range_=range_,
        from_ms=from_ms,
        to_ms=to_ms,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    range_: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
) -> Response[AnalyticsSummary | ErrorBody]:
    """Per-model statistics (runs, tokens, average t/s, P50/P95 TTFT and latency, VRAM,
    error and OOM rates, busy time, last use), client distribution, daily series and the
    latest benchmark of each model. Covers gateway traffic only.

    Args:
        range_ (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AnalyticsSummary | ErrorBody]
    """

    kwargs = _get_kwargs(
        range_=range_,
        from_ms=from_ms,
        to_ms=to_ms,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    range_: str | Unset = UNSET,
    from_ms: int | Unset = UNSET,
    to_ms: int | Unset = UNSET,
) -> AnalyticsSummary | ErrorBody | None:
    """Per-model statistics (runs, tokens, average t/s, P50/P95 TTFT and latency, VRAM,
    error and OOM rates, busy time, last use), client distribution, daily series and the
    latest benchmark of each model. Covers gateway traffic only.

    Args:
        range_ (str | Unset):
        from_ms (int | Unset):
        to_ms (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AnalyticsSummary | ErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
            range_=range_,
            from_ms=from_ms,
            to_ms=to_ms,
        )
    ).parsed
