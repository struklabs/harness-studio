"""Prints server status changes and download progress as they happen (Ctrl+C stops).

    uv run python examples/watch_events.py
"""

from harness_studio import connect, stream_events

client = connect(timeout=None)
try:
    for kind, event in stream_events(client, types=["server_status", "download"]):
        print(kind, event)
except KeyboardInterrupt:
    pass
