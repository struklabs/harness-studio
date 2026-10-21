"""Against a real ``harness serve`` on a throwaway data directory::

    node ../test-server.mjs -- uv run --group test pytest      (from sdk/python)

Without ``HARNESS_SDK_TEST_DATA_DIR`` the server tests are skipped.
"""

from __future__ import annotations

import os
import re
import threading

import pytest

from harness_studio import (
    HarnessError,
    create_client,
    connect,
    discover,
    read_server_sent_events,
    stream_events,
    unwrap,
)
from harness_studio_api.api.analytics import get_analytics_summary, list_requests
from harness_studio_api.api.definitions import get_definition, list_definitions
from harness_studio_api.api.models import list_models
from harness_studio_api.api.system import get_health, get_status
from harness_studio_api.api.tokens import create_token, revoke_token
from harness_studio_api.models import CreateTokenRequest

DATA_DIR = os.environ.get("HARNESS_SDK_TEST_DATA_DIR")
server = pytest.mark.skipif(
    not DATA_DIR, reason="HARNESS_SDK_TEST_DATA_DIR not set (run through sdk/test-server.mjs)"
)


@server
def test_discover_reads_instance_and_token():
    found = discover(DATA_DIR)
    assert re.fullmatch(r"http://127\.0\.0\.1:\d+", found.base_url)
    assert len(found.token) >= 32


@server
def test_health_is_public_status_needs_a_token():
    anonymous = create_client(discover(DATA_DIR).base_url)
    health = unwrap(get_health.sync_detailed(client=anonymous))
    assert health.status == "ok"
    with pytest.raises(HarnessError) as e:
        unwrap(get_status.sync_detailed(client=anonymous))
    assert (e.value.status, e.value.code) == (401, "api.unauthorized")

    status = unwrap(get_status.sync_detailed(client=connect(DATA_DIR)))
    assert status.version == health.version


@server
def test_typed_lists_and_a_structured_404():
    client = connect(DATA_DIR)
    library = unwrap(list_models.sync_detailed(client=client))
    assert library.entries == []
    assert isinstance(unwrap(list_definitions.sync_detailed(client=client)), list)
    with pytest.raises(HarnessError) as e:
        unwrap(get_definition.sync_detailed("nothing-here", client=client))
    assert e.value.status == 404
    assert e.value.message_key == e.value.code


@server
def test_query_parameters_reach_the_server():
    client = connect(DATA_DIR)
    summary = unwrap(get_analytics_summary.sync_detailed(client=client, range_="24h"))
    assert isinstance(summary.models, list)
    with pytest.raises(HarnessError) as e:
        unwrap(get_analytics_summary.sync_detailed(client=client, range_="12w"))
    assert (e.value.status, e.value.code) == (400, "api.invalid_range")
    page = unwrap(list_requests.sync_detailed(client=client, limit=1))
    assert page.total == 0


@server
def test_named_tokens_scopes_and_revocation():
    admin = connect(DATA_DIR)
    created = unwrap(
        create_token.sync_detailed(
            client=admin, body=CreateTokenRequest(name="sdk-py-reader", scopes=["read"])
        )
    )
    reader = create_client(discover(DATA_DIR).base_url, token=created.token)
    unwrap(list_models.sync_detailed(client=reader))
    with pytest.raises(HarnessError) as e:
        unwrap(
            create_token.sync_detailed(
                client=reader, body=CreateTokenRequest(name="nope", scopes=["read"])
            )
        )
    assert (e.value.status, e.value.code) == (403, "api.insufficient_scope")

    unwrap(revoke_token.sync_detailed(created.info.id, client=admin))
    with pytest.raises(HarnessError) as e:
        unwrap(list_models.sync_detailed(client=reader))
    assert e.value.status == 401


@server
def test_event_stream_opens():
    # The stream stays open with keep-alives; reading it from a thread and closing the client
    # ends it. Opening without an error is what is checked here.
    client = connect(DATA_DIR, timeout=None)
    opened = threading.Event()

    def read():
        try:
            for _ in stream_events(client, types=["server_status"]):
                pass
        except Exception:
            pass

    client.get_httpx_client().event_hooks["response"] = [lambda r: opened.set()]
    t = threading.Thread(target=read, daemon=True)
    t.start()
    assert opened.wait(10)
    client.get_httpx_client().close()


def test_read_server_sent_events():
    body = 'event: download\ndata: {"a":1}\n\n: keep-alive\n\nevent: lagged\ndata: 3\r\n\r\ndata: one\ndata: two\n\n'
    events = [(e.event, e.data) for e in read_server_sent_events(iter(body.split("\n")))]
    assert events == [("download", '{"a":1}'), ("lagged", "3"), ("message", "one\ntwo")]


def test_error_without_json_body():
    from http import HTTPStatus

    from harness_studio_api.types import Response

    with pytest.raises(HarnessError) as e:
        unwrap(Response(status_code=HTTPStatus(502), content=b"bad gateway", headers={}, parsed=None))
    assert (e.value.status, e.value.code) == (502, "http.502")


def test_values_from_a_newer_server_are_kept():
    """New enum values and union variants in responses are additive (API stability policy)."""
    from harness_studio_api.models import ServerView
    from harness_studio_api.models.kv_reason import check_kv_reason
    from harness_studio_api.models.server_status_type_0 import check_server_status_type_0

    assert check_kv_reason("too_big") == "too_big"
    assert check_kv_reason("newer_reason") == "newer_reason"
    # A union variant this client does not know comes back as received.
    base = {"id": "a", "name": "A", "is_profile": False}
    assert ServerView.from_dict({**base, "status": "Running"}).status == "Running"
    assert ServerView.from_dict({**base, "status": "Paused"}).status == "Paused"
    # Tags that pick a variant stay strict.
    with pytest.raises(TypeError):
        check_server_status_type_0("Running")
