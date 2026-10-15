from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.automatic_launch import AutomaticLaunch
from ...models.error_body import ErrorBody
from ...types import Response


def _get_kwargs(
    id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/api/harness/v1/models/{id}/automatic".format(
            id=quote(str(id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AutomaticLaunch | None | ErrorBody | None:
    if response.status_code == 200:

        def _parse_response_200(data: object) -> AutomaticLaunch | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                response_200_type_1 = AutomaticLaunch.from_dict(data)

                return response_200_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(AutomaticLaunch | None, data)

        response_200 = _parse_response_200(response.json())

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
) -> Response[AutomaticLaunch | None | ErrorBody]:
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
) -> Response[AutomaticLaunch | None | ErrorBody]:
    """What the default load of a model (context, `-ngl`, slots and KV cache all automatic) starts
    with on this machine: the choice the launch itself makes, so `harness show` and the
    Dashboard's form display what Start does. `null` when the rule does not apply (not a chat
    model).

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AutomaticLaunch | None | ErrorBody]
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
) -> AutomaticLaunch | None | ErrorBody | None:
    """What the default load of a model (context, `-ngl`, slots and KV cache all automatic) starts
    with on this machine: the choice the launch itself makes, so `harness show` and the
    Dashboard's form display what Start does. `null` when the rule does not apply (not a chat
    model).

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AutomaticLaunch | None | ErrorBody
    """

    return sync_detailed(
        id=id,
        client=client,
    ).parsed


async def asyncio_detailed(
    id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[AutomaticLaunch | None | ErrorBody]:
    """What the default load of a model (context, `-ngl`, slots and KV cache all automatic) starts
    with on this machine: the choice the launch itself makes, so `harness show` and the
    Dashboard's form display what Start does. `null` when the rule does not apply (not a chat
    model).

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AutomaticLaunch | None | ErrorBody]
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
) -> AutomaticLaunch | None | ErrorBody | None:
    """What the default load of a model (context, `-ngl`, slots and KV cache all automatic) starts
    with on this machine: the choice the launch itself makes, so `harness show` and the
    Dashboard's form display what Start does. `null` when the rule does not apply (not a chat
    model).

    Args:
        id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AutomaticLaunch | None | ErrorBody
    """

    return (
        await asyncio_detailed(
            id=id,
            client=client,
        )
    ).parsed
