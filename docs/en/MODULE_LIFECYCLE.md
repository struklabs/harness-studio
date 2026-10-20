# Module Lifecycle

[Türkçe](../tr/MODULE_LIFECYCLE.md)

The first 0.2 W3 slice covers Speech Recognition and Hermes Agent. Modules show the installed module and component versions, measured runtime bytes, shared bytes counted separately, estimates, owners, errors, and recovery actions. Enable/Disable keeps files and ownership; disabled modules cannot launch speech workers or connect/run agent tasks. Stop running tasks and backends before changing their runtimes. Stop remains available for an active agent task.

Update appears when the bundled module version advances or a bundled component pin differs from the recorded installation. This is a bundled compatibility contract, not an automatic upstream release tracker. Listing never upgrades version records. Hermes source is pinned to [v0.22.2](https://github.com/NousResearch/hermes-agent/releases/tag/v0.22.2), commit `d923a5eb487a17c0a68801bba9ef0f56c81040a8`. Existing external installations remain usable; Update installs a Harness-managed copy without modifying the external source installation.

## Installation and Recovery

All lifecycle mutations check the manifest contract, platform, and minimum app version. Install/Update checks free space for the complete estimated replacement and downloads, plus a reserve; existing runtime bytes remain on disk as a rollback backup. Repair first runs a bounded runtime verification. A healthy repair works offline. A broken installation is replaced and verified again.

The runtime replacement writes a journal before moving the old runtime to a sibling `.rollback` directory. Installation takes place at the final path so Python environments retain valid absolute paths. Failure/cancellation restores the previous runtime, module state, versions, and ownership. Listing after an interruption replays the journal. A failed rollback retains the journal/backup for another recovery attempt. Shared Python/uv are retained during removal because other optional engines also use them. Repair probes uv and managed Python imports; damaged support directories join the same rollback journal, while healthy Python versions remain intact. Support replacement refuses other module owners or active Python backend processes and is serialized against environment installation. A component with another owner cannot be removed; incompatible shared-version replacement is refused.

Network preflight fails with a localized connection/retry message before moving a working runtime. A later package/network failure also rolls back. Remove, Enable, and Disable require no downloads. Filesystem work, probes, and installation run outside the desktop UI executor. Speech startup is serialized against lifecycle mutations. Process groups are terminated/reaped on stop, timeout, and installer completion.

Hermes uses the official bootstrap's prerequisites/repository/venv/python-deps/complete stages in `<data>/bin/modules/hermes`, with an isolated install home and Python/cache directories. Global command publication, product/UI builds, shell profile changes, setup, and services are skipped. The bootstrap script itself is still fetched from the official HTTPS endpoint and runs with user privileges; the source pin does not hash-pin that bootstrap script. Fresh install and platform end-to-end testing remain necessary before a release.

## Removal and User Data

Removal always preserves models, datasets, projects, and valuable outputs. These are shared user resources, not module-owned components; manage their deletion separately in Library or their original folders. Removing a model record and deleting its physical files remain separate choices in Library.

The module removal confirmation defaults to keeping all user data. Hermes offers separate unchecked **Also Delete Agent History** and **Also Delete Agent Logs** choices with measured sizes. History covers `sessions`, `state.db`, and SQLite sidecars in the detected Hermes home; logs cover `logs`. Selecting these explicitly deletes those files, including history from other Hermes interfaces in that home. Configurations, secrets, skills, memories, projects, and outputs are preserved. External Hermes installations are retained. A running process using the runtime prevents removal; a process using the external runtime also prevents selected history/log deletion.

NSIS hooks only clean Harness's PATH entry and never delete application/user data. Ordinary app uninstall retains the data root and external/custom folders; any installer-provided data-removal choice must be selected explicitly. MSI, macOS app removal, AppImage removal, and `.deb` package removal must not be treated as permission to delete home-directory data. The hooks have a targeted preservation regression check; this slice does not claim installer execution on every platform.

## Validation

Targeted Rust tests exercise shared owners, disk accounting, interrupted replacement, journal replay, disabled state/version restoration, and first-install cleanup while preserving sentinel user files. The download-dependent speech integration test remains opt-in. Frontend action/deletion-default checks live in `scripts/test-modules.mjs`.
