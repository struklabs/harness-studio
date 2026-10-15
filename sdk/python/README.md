# harness-studio-sdk (Python)

Client of the [Harness Studio](https://github.com/struklabs/harness-studio) Control API for Python 3.10+, generated from the API's OpenAPI document with [openapi-python-client](https://github.com/openapi-generators/openapi-python-client) (httpx and attrs; sync and asyncio).

Not on PyPI yet. Install it from the repository:

```bash
pip install ./sdk/python        # or: uv pip install ./sdk/python
```

The generated part (`harness_studio_api`) is not committed: the build generates it from `sdk/openapi.json`, which needs Node.js and [uv](https://docs.astral.sh/uv/).

## Use

```python
from harness_studio import connect, unwrap, HarnessError
from harness_studio_api.api.models import list_models
from harness_studio_api.api.servers import load_model
from harness_studio_api.api.system import get_status
from harness_studio_api.models import LoadRequest

# Finds the running app or `harness serve` through <data dir>/instance.json
# (HARNESS_DATA_DIR, else the app's folder) and uses the session token.
client = connect()

status = unwrap(get_status.sync_detailed(client=client))
print(status.version, status.servers.running)

library = unwrap(list_models.sync_detailed(client=client))
try:
    unwrap(load_model.sync_detailed(client=client, body=LoadRequest(alias=library.definitions[0].alias)))
except HarnessError as e:
    print(e.code, e.params)
```

- Two packages: `harness_studio_api` is generated (one module per operation, grouped by tag: `harness_studio_api.api.<tag>.<operation>` with `sync`, `sync_detailed`, `asyncio`, `asyncio_detailed`; models in `harness_studio_api.models`). `harness_studio` is the small hand-written layer.
- `create_client(base_url, token=...)` connects to a given address (another computer, a named token); `connect(data_dir=None)` and `discover(data_dir=None)` find the local instance.
- `unwrap(response)` returns the parsed answer of a `*_detailed` call or raises `HarnessError` with `status`, `code` (`api.not_found`, `api.insufficient_scope`...), `message_key` and `params`.
- `stream_events(client, types=[...])` yields the event bus (`GET /events`) as `(type, event)`.

Examples: [`examples/`](examples/).

## Development

`src/harness_studio_api/` is generated from `sdk/openapi.json` and not committed: `uv sync`, a build and the pytest session generate it (`node sdk/generate.mjs --from-snapshot --skip-typescript`). Tests: see [`sdk/README.md`](../README.md#tests).
