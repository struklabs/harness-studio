# Run Harness as a Linux Server

The headless server runs the same Control API, model services, and browser administration as `harness serve`, without Tauri, WebKitGTK, a display server, or Node.js at runtime. The server owns its data directory; do not open the desktop app against that directory at the same time.

## Install on Debian or Ubuntu

Headless artifacts are optional release assets, separate from the desktop `.deb`. When a release includes `harness-server-<version>-linux-x86_64.deb` (or `aarch64`), install with:

```sh
curl -fsSL https://raw.githubusercontent.com/struklabs/harness-studio/main/packaging/server/install.sh | sudo sh
```

The script downloads the latest stable release's server package and verifies the SHA-256 digest supplied by GitHub release metadata. If that release has no server asset or digest, installation stops. This is a digest check over HTTPS, not the desktop updater's signing-key verification. Review the script first or pin its URL to a trusted commit. For an already downloaded package:

```sh
sudo apt-get install ./harness-server-0.2.0-linux-x86_64.deb
# Equivalent installer entry point when dependencies are already installed:
sudo sh packaging/server/install.sh ./harness-server-0.2.0-linux-x86_64.deb
```

The installer creates the `harness` system account, stores data in `/var/lib/harness` with mode `0700`, and starts `harness-server.service`. Program files are root-owned under `/usr/lib/harness`; the browser configuration is `/etc/harness/serve.json`. Installation prints the browser URL and a one-time setup code, valid for 15 minutes. The code is kept in the private `/var/lib/harness/setup-code` file instead of the service journal; after setup, the next restart removes that file. Only a server administrator should read it:

```sh
sudo cat /var/lib/harness/setup-code
```

Open `http://localhost:11500/` on the server, verify the server address, and set an administrator password of at least 12 characters. For a remote machine, use an SSH tunnel first:

```sh
ssh -L 11500:127.0.0.1:11500 user@server
```

Then open `http://localhost:11500/` on your computer. No public network listener is needed. For direct LAN/Internet access, configure trusted TLS, the exact public origin, network approval, and a named token as described in [Browser Management](../../en/CLI.md#browser-management). Plain remote HTTP cannot accept password login. The service reads `/etc/harness/serve.json`; changing its launch arguments requires a systemd override. The inference Gateway has separate network and authentication settings.

## Build from Source or Install the Tar Archive

Until headless release assets are published, build on Linux with Rust, Node.js 22.18 or later, Python 3, `dpkg-deb`, `pkg-config`, OpenSSL development headers, and D-Bus development headers:

```sh
npm ci
npm run build
cargo build --locked --release -p harness-cli
version=$(node -p "require('./package.json').version")
python3 packaging/server/package.py --binary target/release/harness --ui build --output target/server --version "$version"
sudo sh packaging/server/install.sh "target/server/harness-server-$version-linux-$(uname -m).deb"
```

The same command produces a `.tar.gz` containing the binary, complete static UI, service unit, helper, and installer. On a systemd distribution with Python 3, `sudo`, `adduser`, OpenSSL 3, D-Bus, and CA certificates, extract the trusted tarball into an empty directory and run `sudo sh ./install.sh` there. Use a Linux build of the binary for the target architecture. No model runtime or model weights are bundled: install the appropriate inference runtime and models through Harness when needed.

## Server Operations in the Browser

Sign in and open **Server Operations**. These routes require administrator access, including their read operations; cookie writes require CSRF verification.

- **Service Lifecycle** shows the program version, uptime, and start-on-boot choice. Restart stops running models, releases the data lock, and ends browser sessions. Sign in again afterward. The Linux package delegates only fixed operations to a root-owned helper through a dedicated sudo rule. Foreground and Docker owners show the service controls as unavailable; use the terminal or Compose for their lifecycle.
- **Server Updates** checks the existing `struklabs/harness-studio` stable release feed. Apply is available only for a newer stable version with an official server `.deb` for this architecture and a GitHub SHA-256 digest. It rechecks the selected version, downloads and verifies before installation, then installs through a separate systemd job that survives server restart. Back up first. Container and tar installations use manual updates. There is no automatic package rollback: reinstall a retained trusted package if necessary, retaining `/var/lib/harness`. Check `systemctl list-units 'run-*'` and the journal if an accepted package update fails.
- **Service Logs** reads the last 500 journal lines and downloads exactly that displayed text. Logs can contain private paths and error details: review before sharing. Model process logs remain available through the existing server logs API/CLI. For Docker use `docker compose logs`; for foreground serving capture terminal output.
- **Backup and Restore** downloads a password-encrypted JSON file. Use a separate password of at least 12 characters. Encryption uses Argon2id and ChaCha20-Poly1305, with fresh random salt and nonce for each backup. Wrong passwords, modified ciphertext, unknown formats, unsupported schemas, and configuration payloads above 256 KiB are rejected.

Backups include the current supported versions of `settings.json`, `model_definitions.json`, `server_profiles.json`, and `inference_presets.json`, plus named-token metadata (names, scopes, policies, timestamps). Inline values in these configuration files are encrypted too. There is no recursive filesystem traversal. Model files, downloads, chats, generated outputs, runtime binaries, browser password hashes, active sessions, API token secrets/hashes, TLS keys, and OS keychain secrets are excluded. Save valuable files separately. A token inventory does not restore working tokens: recreate them and update clients after moving to a new server. Existing tokens and the browser password on the destination are preserved. Reenter any missing keychain credentials and verify machine-specific paths.

Restore stages a private configuration snapshot under `/var/lib/harness`; restart to apply it. A second restore is refused until the pending one is applied. Startup acquires the exclusive owner lock, validates the file allowlist and schemas, and retains a private rollback journal until the restore completes. A failed restore stops startup instead of serving partly restored configuration; fix the filesystem error and restart to retry. The pending plaintext and rollback files have owner-only permissions and are removed on success. Protect the server data directory as carefully as the encrypted download. The running listener keeps its configured bind during this startup; restart again if you deliberately restored different Control API listener defaults. Reconnecting always requires a fresh browser session.

For service diagnosis:

```sh
sudo systemctl status harness-server
sudo journalctl -u harness-server -n 100 --no-pager
sudo systemctl restart harness-server
sudo systemctl enable harness-server   # or disable; does not stop it
```

Removing or purging `harness-server` stops the service but preserves the data directory and service account. Move or remove those separately only after saving your data. Do not run `harness service install` for this system package: that command creates a separate user-login service.

## Docker: CPU and Vulkan

Build from this checkout; these commands do not assume a published container registry image:

```sh
docker build -f packaging/server/Dockerfile --target cpu -t harness-server:cpu .
docker compose -f packaging/server/compose.yaml up -d
docker compose -f packaging/server/compose.yaml logs
```

The example uses Linux host networking, a persistent named volume at `/var/lib/harness`, an unprivileged UID/GID `10001`, and a loopback-only listener. Read the setup code from the initial container logs. Docker Desktop must have host networking enabled to use this example; otherwise run it on Linux. For a published port instead, configure TLS, network approval, and a network listener first. Simply publishing a port cannot reach a loopback-only container listener.

For Vulkan, build with `--target vulkan` and change the Compose build target. Pass `/dev/dri` and the host's numeric render/video group IDs using `devices` and `group_add`. The image adds the Vulkan loader and Mesa drivers; the inference runtime and host GPU still need to support Vulkan. CUDA and ROCm images and hardware verification remain future work. No GPU throughput is implied by a successful image build.

Restart with `docker compose -f packaging/server/compose.yaml restart`. Update by rebuilding the image from the desired trusted version and recreating the container. The named volume retains administrator setup, configuration, model files, and token state. Browser backup/restore works the same way; restart the container to apply a staged restore. `docker compose down` preserves the volume; `down --volumes` deletes it.

## CLI and SDK Clients

Use [scoped API tokens](09-cli-api-mcp.md) for remote scripts and SDKs. A browser session does not reveal the local owner token. Local root administration can point the CLI at the service data directory with `sudo -u harness harness --data-dir /var/lib/harness ...` after adding `/usr/lib/harness` to PATH, or use the executable's full path.

Publication checked on October 20, 2026: [npm publishes `@struklabs/harness-studio-sdk` at 0.1.0](https://registry.npmjs.org/@struklabs%2fharness-studio-sdk/latest); [`harness-studio-sdk` is not available from PyPI](https://pypi.org/project/harness-studio-sdk/). Use `npm install @struklabs/harness-studio-sdk` for that published version. Build the TypeScript client from this checkout for new server operations added after its publication. Python remains source-only: `pip install ./sdk/python`, with Node.js and uv available for generation. Do not imply that a source checkout's new API coverage has already been published.

Both source clients are generated from `sdk/openapi.json`: TypeScript exposes typed HTTP paths through openapi-fetch, and Python generates synchronous/asynchronous endpoint modules. Generation fails on Python schema warnings. SSE parsing is supplied separately; MCP's JSON-RPC endpoint is outside the OpenAPI HTTP clients. See [SDK setup and coverage](../../../sdk/README.md).
