"""Loads a model by its definition alias and prints where it is served.

    uv run python examples/load_model.py <alias>
"""

import sys

from harness_studio import HarnessError, connect, unwrap
from harness_studio_api.api.servers import load_model
from harness_studio_api.models import LoadRequest

if len(sys.argv) != 2:
    sys.exit("usage: load_model.py <definition alias>")
client = connect(timeout=600)
try:
    loaded = unwrap(load_model.sync_detailed(client=client, body=LoadRequest(alias=sys.argv[1])))
except HarnessError as e:
    sys.exit(f"could not load: {e}")
print(loaded.to_dict())
