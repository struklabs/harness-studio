"""Prints the running Harness Studio's version, its servers and the library.

    uv run python examples/status.py

Finds the instance through <data dir>/instance.json (HARNESS_DATA_DIR or the app's folder).
"""

from harness_studio import connect, unwrap
from harness_studio_api.api.models import list_models
from harness_studio_api.api.servers import list_servers
from harness_studio_api.api.system import get_status

client = connect()
status = unwrap(get_status.sync_detailed(client=client))
print(f"Harness Studio {status.version}: {status.servers.running} of {status.servers.total} servers running")
for server in unwrap(list_servers.sync_detailed(client=client)):
    print(f"server {server.name}: {server.status}")
library = unwrap(list_models.sync_detailed(client=client))
for entry in library.entries:
    print(f"model {entry.model.id}")
for definition in library.definitions:
    print(f"definition {definition.alias}")
