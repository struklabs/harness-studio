# Modules

Modules are optional capabilities (speech recognition, agents) that Harness Studio installs on demand. Open **Modules** to see each module's functions, platform and app-version requirements, components, disk use, and permissions. Installing from the internet downloads only the backend components; models and your data are never part of a module.

Removing a module keeps models, outputs, datasets, projects, and the shared Python and uv tools. Removal never needs the network.

## Offline Install and Export

A module can be installed with no network connection from a `.hsmod` bundle.

- **Install from File…** (Modules, top right) lets you pick a bundle. Nothing is downloaded.
- `harness modules import bundle.hsmod` does the same from the command line.
- `harness modules export <id> --out bundle.hsmod` packs an installed module into a bundle (see the [CLI reference](CLI.md#modules)).

### Official bundles only

Harness Studio installs a bundle only if it is an **official** one. The whole `.hsmod` file must have the SHA-256 recorded in the index shipped inside the app (`crates/harness-core/src/modules/official_bundles.json`), for exactly that module id and version, and the module version must be the one this app version installs. Anything else (a changed file, another version, a bundle someone else built) is refused with a message, before any file is written. There are no community packages.

### What happens during an import

1. The file's SHA-256 is compared with the official index. No match: refused.
2. The bundle manifest is read; module id, version, platform, and components must match the app's own module definition. Every install folder must be one the component is allowed to use.
3. Every file is extracted to a temporary folder next to the live folders and checked against its recorded SHA-256 and size. A single mismatch refuses the whole bundle.
4. Only then are the live folders changed, through the same journal the online installer uses. If anything fails (including the final check that the module starts), the previous state is restored: no half-installed module and the shared-component owner lists stay as they were.
5. Shared components (the managed Python and uv) that are already installed are reused, not copied again; the module is only added to their owners. Removing the module later keeps them while another module still uses them.

A bundle is made for one platform (for example `windows-x86_64`) and has to be imported on the same platform. If the data folder is in another place on the target computer, the virtual environment's `pyvenv.cfg` is pointed to the new location automatically.

### Bundle format (version 1)

A ZIP archive:

- `manifest.json`: format version, module id and version, platform, the bin folder it was made in, and for each component its install folders, every file with `sha256` and `size`, and any symbolic links.
- `payload/<path>`: the files, with paths relative to the app's `bin` folder.

Export is deterministic (sorted entries, fixed timestamps), so exporting the same install twice gives the same file and the same hash. Extraction rejects absolute paths, `..`, drive letters, and links that leave the data folder.

### Recording an official hash (maintainers)

1. Install the module on a machine of the target platform from the pinned online sources and check that it works.
2. Run `harness modules export <id> --out <id>-<version>-<platform>.hsmod`. The command prints the bundle's `sha256`.
3. Publish the file and add an entry to `crates/harness-core/src/modules/official_bundles.json`:

   ```json
   [{ "id": "speech", "version": "0.1.0", "sha256": "<the printed hash>" }]
   ```

   Add one entry per platform. Entries must name a bundled, non-planned module at its current version; a unit test checks this.
4. Ship the app. Only builds that contain the entry accept that bundle. The index is empty until hashes are recorded, so until then every import is refused with `modules.bundle_unofficial`; do this before announcing offline install in a release.

Modules that use an agent program installed outside Harness Studio cannot be exported.
