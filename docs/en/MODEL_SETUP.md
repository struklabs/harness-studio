# Discover and Model Setup

The Discover category bar keeps Models in the existing Model Hub. Skills, MCP Servers,
and Tools open the existing official Registry with the matching type selected. Bundled
Modules open the existing Modules workspace. Module manifests, backend dependencies,
installation state, and disk accounting remain separate from the official Registry;
the Registry still accepts only Skills, MCP servers, and Tools.

Connectors have a separate category explaining account/data connections and linking
to existing MCP connection settings. There is no dedicated Connector catalog or new
OAuth installation flow in this slice. MCP permissions and registry installation
review continue to use the existing screens.

Playground and Library empty states point to model discovery and import. The model
picker separates models on this PC from remote Harness models, shows execution
location even when a remote link is offline, and explains model switching and server
replacement. Library's Review Resources action opens that model's existing Playground
load settings without loading it. A chat file with no saved definition opens the
existing Dashboard setup form so its resources can be reviewed before its first load.

For llama.cpp models, load settings offer saved resources, automatic CPU/GPU selection,
CPU only, and individual devices reported by the runtime when its device-selection
flags are supported. Individual selection disables multi-device splitting. Automatic
selection clears saved GPU and split restrictions. The same override map is used for
memory estimation, loading, and saving. Simple mode retains its small form; other
engines continue to use their existing engine-specific settings.

Memory fit is an estimate, with explicit pending and unavailable states. Device probe
failure leaves saved settings and CPU selection available. Choosing a server chooses
a local process, not a physical device. Remote Harness models execute on the linked
machine and do not expose local resource controls.

Models, providers, agents, workflows, and devices are distinct concepts in the picker
help. This Playground currently supports local and linked Harness models; direct
provider model connections are not implemented by this slice. Agent and workflow
lifecycle features remain in their own workspaces or roadmap waves.
