from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.core_event_type_0 import CoreEventType0
from ...models.core_event_type_1 import CoreEventType1
from ...models.core_event_type_2 import CoreEventType2
from ...models.core_event_type_3 import CoreEventType3
from ...models.core_event_type_4 import CoreEventType4
from ...models.core_event_type_5 import CoreEventType5
from ...models.core_event_type_6 import CoreEventType6
from ...models.core_event_type_7 import CoreEventType7
from ...models.core_event_type_8 import CoreEventType8
from ...models.core_event_type_9 import CoreEventType9
from ...models.core_event_type_10 import CoreEventType10
from ...models.core_event_type_11 import CoreEventType11
from ...models.core_event_type_12 import CoreEventType12
from ...models.core_event_type_13 import CoreEventType13
from ...models.core_event_type_14 import CoreEventType14
from ...models.error_body import ErrorBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    types: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["types"] = types

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/events",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> (
    CoreEventType0
    | CoreEventType1
    | CoreEventType10
    | CoreEventType11
    | CoreEventType12
    | CoreEventType13
    | CoreEventType14
    | CoreEventType2
    | CoreEventType3
    | CoreEventType4
    | CoreEventType5
    | CoreEventType6
    | CoreEventType7
    | CoreEventType8
    | CoreEventType9
    | ErrorBody
    | None
):
    if response.status_code == 200:

        def _parse_response_200(
            data: object,
        ) -> (
            CoreEventType0
            | CoreEventType1
            | CoreEventType10
            | CoreEventType11
            | CoreEventType12
            | CoreEventType13
            | CoreEventType14
            | CoreEventType2
            | CoreEventType3
            | CoreEventType4
            | CoreEventType5
            | CoreEventType6
            | CoreEventType7
            | CoreEventType8
            | CoreEventType9
        ):
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_0 = CoreEventType0.from_dict(data)

                return componentsschemas_core_event_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_1 = CoreEventType1.from_dict(data)

                return componentsschemas_core_event_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_2 = CoreEventType2.from_dict(data)

                return componentsschemas_core_event_type_2
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_3 = CoreEventType3.from_dict(data)

                return componentsschemas_core_event_type_3
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_4 = CoreEventType4.from_dict(data)

                return componentsschemas_core_event_type_4
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_5 = CoreEventType5.from_dict(data)

                return componentsschemas_core_event_type_5
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_6 = CoreEventType6.from_dict(data)

                return componentsschemas_core_event_type_6
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_7 = CoreEventType7.from_dict(data)

                return componentsschemas_core_event_type_7
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_8 = CoreEventType8.from_dict(data)

                return componentsschemas_core_event_type_8
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_9 = CoreEventType9.from_dict(data)

                return componentsschemas_core_event_type_9
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_10 = CoreEventType10.from_dict(data)

                return componentsschemas_core_event_type_10
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_11 = CoreEventType11.from_dict(data)

                return componentsschemas_core_event_type_11
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_12 = CoreEventType12.from_dict(data)

                return componentsschemas_core_event_type_12
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_core_event_type_13 = CoreEventType13.from_dict(data)

                return componentsschemas_core_event_type_13
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_core_event_type_14 = CoreEventType14.from_dict(data)

            return componentsschemas_core_event_type_14

        response_200 = _parse_response_200(response.text)

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
) -> Response[
    CoreEventType0
    | CoreEventType1
    | CoreEventType10
    | CoreEventType11
    | CoreEventType12
    | CoreEventType13
    | CoreEventType14
    | CoreEventType2
    | CoreEventType3
    | CoreEventType4
    | CoreEventType5
    | CoreEventType6
    | CoreEventType7
    | CoreEventType8
    | CoreEventType9
    | ErrorBody
]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    types: str | Unset = UNSET,
) -> Response[
    CoreEventType0
    | CoreEventType1
    | CoreEventType10
    | CoreEventType11
    | CoreEventType12
    | CoreEventType13
    | CoreEventType14
    | CoreEventType2
    | CoreEventType3
    | CoreEventType4
    | CoreEventType5
    | CoreEventType6
    | CoreEventType7
    | CoreEventType8
    | CoreEventType9
    | ErrorBody
]:
    """Server status changes, download progress (at most ~2 per second per job) and benchmark
    progress, as they happen. Each SSE `event:` is the type, `data:` the JSON event. A client
    that falls behind gets an `event: lagged` with the number of skipped events.

    Args:
        types (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CoreEventType0 | CoreEventType1 | CoreEventType10 | CoreEventType11 | CoreEventType12 | CoreEventType13 | CoreEventType14 | CoreEventType2 | CoreEventType3 | CoreEventType4 | CoreEventType5 | CoreEventType6 | CoreEventType7 | CoreEventType8 | CoreEventType9 | ErrorBody]
    """

    kwargs = _get_kwargs(
        types=types,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    types: str | Unset = UNSET,
) -> (
    CoreEventType0
    | CoreEventType1
    | CoreEventType10
    | CoreEventType11
    | CoreEventType12
    | CoreEventType13
    | CoreEventType14
    | CoreEventType2
    | CoreEventType3
    | CoreEventType4
    | CoreEventType5
    | CoreEventType6
    | CoreEventType7
    | CoreEventType8
    | CoreEventType9
    | ErrorBody
    | None
):
    """Server status changes, download progress (at most ~2 per second per job) and benchmark
    progress, as they happen. Each SSE `event:` is the type, `data:` the JSON event. A client
    that falls behind gets an `event: lagged` with the number of skipped events.

    Args:
        types (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CoreEventType0 | CoreEventType1 | CoreEventType10 | CoreEventType11 | CoreEventType12 | CoreEventType13 | CoreEventType14 | CoreEventType2 | CoreEventType3 | CoreEventType4 | CoreEventType5 | CoreEventType6 | CoreEventType7 | CoreEventType8 | CoreEventType9 | ErrorBody
    """

    return sync_detailed(
        client=client,
        types=types,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    types: str | Unset = UNSET,
) -> Response[
    CoreEventType0
    | CoreEventType1
    | CoreEventType10
    | CoreEventType11
    | CoreEventType12
    | CoreEventType13
    | CoreEventType14
    | CoreEventType2
    | CoreEventType3
    | CoreEventType4
    | CoreEventType5
    | CoreEventType6
    | CoreEventType7
    | CoreEventType8
    | CoreEventType9
    | ErrorBody
]:
    """Server status changes, download progress (at most ~2 per second per job) and benchmark
    progress, as they happen. Each SSE `event:` is the type, `data:` the JSON event. A client
    that falls behind gets an `event: lagged` with the number of skipped events.

    Args:
        types (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CoreEventType0 | CoreEventType1 | CoreEventType10 | CoreEventType11 | CoreEventType12 | CoreEventType13 | CoreEventType14 | CoreEventType2 | CoreEventType3 | CoreEventType4 | CoreEventType5 | CoreEventType6 | CoreEventType7 | CoreEventType8 | CoreEventType9 | ErrorBody]
    """

    kwargs = _get_kwargs(
        types=types,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    types: str | Unset = UNSET,
) -> (
    CoreEventType0
    | CoreEventType1
    | CoreEventType10
    | CoreEventType11
    | CoreEventType12
    | CoreEventType13
    | CoreEventType14
    | CoreEventType2
    | CoreEventType3
    | CoreEventType4
    | CoreEventType5
    | CoreEventType6
    | CoreEventType7
    | CoreEventType8
    | CoreEventType9
    | ErrorBody
    | None
):
    """Server status changes, download progress (at most ~2 per second per job) and benchmark
    progress, as they happen. Each SSE `event:` is the type, `data:` the JSON event. A client
    that falls behind gets an `event: lagged` with the number of skipped events.

    Args:
        types (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CoreEventType0 | CoreEventType1 | CoreEventType10 | CoreEventType11 | CoreEventType12 | CoreEventType13 | CoreEventType14 | CoreEventType2 | CoreEventType3 | CoreEventType4 | CoreEventType5 | CoreEventType6 | CoreEventType7 | CoreEventType8 | CoreEventType9 | ErrorBody
    """

    return (
        await asyncio_detailed(
            client=client,
            types=types,
        )
    ).parsed
