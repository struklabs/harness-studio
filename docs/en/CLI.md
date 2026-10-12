Türkçe: ../tr/CLI.md

# The `harness` CLI and headless mode

`harness` is Harness Studio's command-line interface. It uses the same core (`harness-core`) and the same Control API (`harness-api`) as the GUI; there is no separate business logic. Source: `crates/harness-cli`.

```
cargo build -p harness-cli --release     # target/release/harness(.exe)
harness --help
```

## Installation and PATH

Installing the desktop app places `harness` next to the application (a Tauri sidecar: on Windows/Linux in the same folder as `harness-studio(.exe)`, on macOS at `Harness Studio.app/Contents/MacOS/harness`; in the Linux `.deb` package at `/usr/bin/harness`, which is already on PATH). To make it work from any terminal, use the **Add to PATH** button in **Settings > Developer > CLI > Use in a Terminal**; it acts only when you click it, and **Remove** reverses it:

- **Windows:** The application's folder is added to the user PATH (`HKCU\Environment\Path`, the value type is preserved, other entries are untouched), and `WM_SETTINGCHANGE` is broadcast. Open terminals do not see the change; open a new terminal.
- **macOS/Linux:** A `~/.local/bin/harness` symbolic link is created. If `~/.local/bin` is not on PATH, the UI shows the line `export PATH="$HOME/.local/bin:$PATH"`. On removal only a link pointing to this CLI is deleted; if another `harness` file exists, adding is refused.

CLI lookup order (`harness_core::integrations::cli_install::locate`; the Harness MCP setup, diagnostics, and this section use the same function): next to the running application (sidecar; in a development build, the `harness` built into the same folder), then the first `harness` on PATH, then the path the user entered.

## How it works: discovery

Every command talks to a Control API. Which one is decided in this order:

1. If **`--host` / `HARNESS_HOST`** is given, that remote API. The token is given with `--token` / `HARNESS_TOKEN` (a named token; scopes apply).
2. **If a running owner exists** (the GUI or `harness serve`): `instance.json` in the data directory is read, and the API address and session token file are taken from it. The command goes through the owner's API; the CLI never writes to another process's data directory.
3. **If an owner exists but its Control API is off** (Settings > Developer > Control API is off in the GUI): exit code 4, `cli.owner_without_api`.
4. **If there is no owner** (or `instance.json` is left over from a crash): the command opens a **temporary instance** inside this process. The temporary instance takes `instance.lock`, starts the same Control API on a random loopback port, closes everything it started when the command ends, and releases the lock. `--json` output therefore has the same schema on every path.
   - Commands that start llama-server (`load`, `run`, `chat`, `profile run`) open the temporary instance only with **`--headless`**; in that case the servers they start stop when the command ends.
   - Commands that work on existing servers (`stop`, `unload`, `logs`, `shutdown`, `pull --detach`) do not open a temporary instance; they need a running instance (otherwise exit 6).

`status`, `doctor`, `search`, `speak`, `tts`, `transcribe`, `asr`, `completion`, and `service` need no instance and take no lock. `status` and `doctor` only read (even the capability probe is not cached to disk).

### Data directory

The default is the same as the GUI's (Tauri `app_data_dir`):

| OS | Path |
|---|---|
| Windows | `%APPDATA%\com.harnessstudio.app` |
| macOS | `~/Library/Application Support/com.harnessstudio.app` |
| Linux | `$XDG_DATA_HOME/com.harnessstudio.app` (default `~/.local/share/...`) |

It is changed with `--data-dir` / `HARNESS_DATA_DIR`. Tests always use a temporary directory.

## Global options

| Option | Environment variable | Meaning |
|---|---|---|
| `--json` | | Data goes to stdout as JSON; errors go to stderr as `{"error": {"code", "message", "exit_code", "params"}}` |
| `-y`, `--yes` | | Answers confirmations with yes. With no terminal, destructive commands stop with exit 2 unless `--yes` is given |
| `-q`, `--quiet` | | Turns off progress lines on stderr |
| `--data-dir DIR` | `HARNESS_DATA_DIR` | Data directory |
| `--host HOST` | `HARNESS_HOST` | Control API of a remote instance (`host:port` or a URL; 11500 if there is no port) |
| `--token TOKEN` | `HARNESS_TOKEN` | The token for `--host` |
| `--headless` | | A temporary instance for the duration of the command if no instance is running |
| | `HARNESS_MEMORY_SECRETS=1` | An in-memory secret store instead of the OS keychain (tests and isolated runs) |
| | `HARNESS_SERVER_API_KEY` | `--api-key` for `run`/`chat` against a llama-server that has a key |

stdout carries data only; progress, warnings, and errors go to stderr. Progress lines update in place on a terminal and are written as sparse lines when there is no terminal.

## Exit codes

| Code | Meaning | Example |
|---|---|---|
| 0 | Success | |
| 1 | General error | token rejected (`api.unauthorized`, `api.insufficient_scope`), the server could not start (`cli.server_failed`, `servers.crashed`, `servers.corrupt_model`, `servers.missing_library`, `servers.unsupported_architecture`, `servers.invalid_argument`, `servers.load_timeout`...), download failed |
| 2 | Usage error | unknown command/option, invalid `owner/repo`, unknown scope, ambiguous model name, confirmation with no terminal (`cli.confirmation_required`) |
| 3 | Not found | model, profile, server, benchmark, token, a model file llama-server could not open (`servers.model_not_found`, `cli.not_found`, `api.not_found`, `models.not_found`, `definitions.alias_not_found`, `downloads.variant_not_found`) |
| 4 | Conflict | the data directory is in another process (`cli.lock_held`, `cli.owner_without_api`), port in use (`api.bind_failed`, `servers.port_in_use` and `servers.port_in_use_by` / `_at` / `_server`: before launch or when llama-server could not bind; the message names the program holding the port), a profile with the same id (`servers.exists`), model in use (`models.in_use`), the server is already running, a keyless network bind (`network.lan_without_key`) |
| 5 | Out of memory | llama-server reported an allocation error while loading (`servers.out_of_memory`; without a classification, `cli.out_of_memory`: `ErrorOutOfDeviceMemory`, `failed to allocate`, `cudaMalloc failed`, `out of memory`...). In `--json` errors, the `failure` and `recovery` (recovery options) fields; in text output, `harness retry ...` suggestions |
| 6 | Connection error | the API could not be reached (`cli.connection_failed`), no running instance (`cli.no_instance`), the llama-server endpoint could not be reached |

HTTP statuses map as follows: 400 → 2, 404 → 3, 409 → 4, 401/403/422/500 → 1, connection error → 6.

## Error messages and language

CLI output is in English. Core and API errors return an i18n key (`models.in_use`, `api.not_found`...):

- For the keys the GUI knows (`src/lib/network.ts` → `ERROR_KEYS`), the CLI text is **identical** to the GUI's English translation; a unit test compares it against `src/lib/locales/*.ts`, so the texts cannot drift apart.
- Only the API's and the CLI's own codes (`api.unauthorized`, `cli.no_instance`...) are in `crates/harness-cli/src/messages.rs`.
- The core's other errors are keys too (`harness_core::error_key`: `key {"name": "value"}`); the English text is taken from the GUI's error locale files (`src/lib/locales/errors*.ts`, embedded in the build), and `{name}` placeholders are filled with the parameters. Parameters carry technical detail (a path, an operating system error text) untranslated; a parameter can itself be a nested keyed error, which is translated too. A test checks that every `err("…")` key in the core has a text.
- Raw operating system/library errors with no context are still free text (`core.error`, `params.message`) and are written as they are.
- An unknown key is never left empty: the key itself and its parameters are written.
- In `--json`, `code` is always the machine code; do not branch on the text.

## Commands

Unless noted otherwise, `--json` output is the relevant Control API response itself; the schema is at `GET /api/harness/v1/openapi.json`.

### Status

| Command | What it does | `--json` |
|---|---|---|
| `status` | The data directory, the owner (none / running / API off / stale), the owner's `/status` summary (gateway line: is it running, its address, is a token required); if there is no owner, the resolved runtime | `{data_dir, owner: {state, info}, via, api_url, api, runtime}` (`api.gateway`: `{running, host, port, endpoint, require_token, error, ollama_port, ollama_warning}`); with `--host`, `{via: "remote", api_url, api}` |
| `doctor` | Checks (`id`): `data_dir` (writable, and whether the token file is open only to the user), `instance` (the owner), `runtime` (path and version), `runtime_compat` (the recorded vs. actual build, a staging build that has not been activated, falling back to a binary on the system instead of the configured runtime), `gpu_libraries` (Windows: `vulkan-1.dll`, `nvcuda.dll`, `amdhip64*.dll`; Linux: `ldconfig -p` and known directories; macOS: Metal; the `ggml-*` backends in the runtime folder), `gpu_devices` (`--list-devices`), `hardware`, `ports` (are the Control API, default server, and profile ports taken; do two profiles use the same port; profiles with automatic ports are skipped). The same report is in the GUI at Settings > Diagnostics | `{ok, data_dir, version, checks: [{id, status: ok\|warn\|fail, summary, message_key, params, detail}], runtime}`. `summary` is English text; `message_key` (`doctor.*`) and `params` are stable. If a check is `fail`, exit 1 |

### Models

| Command | What it does | `--json` |
|---|---|---|
| `ls` (`list`) | Local models and the model definitions on them | `GET /models` |
| `show <model>` | File, type, architecture, quant, size, context, parameters, parts, and definitions; the `memory:` line has the memory estimate on this machine (default context, full offload; `measured`/`corrected` if there is a measurement from an earlier load) and the recommended `-ngl`; the `default load:` line is what a default load starts with (context, `-ngl`, `-np`, KV types, flash attention; `GET /models/{id}/automatic`, the same rule the launch and the Dashboard form use). `<model>`: a library id/path, file name, name, or a definition alias | `{model: LibraryEntry\|null, definitions: [...], fit: FitResult\|null, automatic: AutomaticLaunch\|null}` (`GET /models/{id}/fit`) |
| `search <q> [--limit N] [--sort S]` | GGUF search on Hugging Face (no instance needed; the HF token is read from the keychain) | `HfModelSearchResult[]` |
| `pull <owner/repo[:quant]> [--file F] [--detach]` | Downloads and shows progress until done. `hf.co/...` and `https://huggingface.co/...` are also accepted. Ctrl+C pauses the job (`pull` again resumes it). `--detach` queues it and returns (needs a running instance). If an image model from the ready list (FLUX, SD3.5) is pulled, the missing VAE and text encoder files are also downloaded and waited for (`companions`); the model definition is created automatically once everything arrives | `{job_id, variant, companions, download}`; `--detach`: the `POST /models/pull` response |
| `rm <model> [--definitions] [-y]` | Deletes all of the model's files (4 if it is used on a server). `--definitions` also deletes the definitions on it | `DeleteReport` |

### Hugging Face account

No instance needed; the token lives in the OS keychain, and the GUI, `harness serve`, `search`, and `pull` use the same one (an OAuth token is refreshed first if needed).

| Command | What it does | `--json` |
|---|---|---|
| `hf login [--client-id ID] [--no-browser]` | Opens huggingface.co in the browser and waits for approval at `127.0.0.1:<random port>/callback` (OAuth + PKCE, at most 5 min). The address is also written to stderr. Client ID: `--client-id`, Settings, or `HARNESS_HF_OAUTH_CLIENT_ID`; if there is none, it prints what to register and exits with 1 | `HfAuthStatus` (no token) |
| `hf login --device [--client-id ID]` | Device-code flow: the code and address are written to stderr, and it waits until approved on huggingface.co (for machines without a browser) | `HfAuthStatus` |
| `hf login --with-token` | Reads the access token from stdin (`harness hf login --with-token < token.txt`; on a terminal, a single line, and what you type is visible), validates it with `whoami-v2` first, then saves it. It replaces an OAuth session | `{kind: "token", user}` |
| `hf logout` | Deletes the token and the OAuth session from the keychain; prints the page where the grant can be revoked | `{logged_out, revoke_at}` |
| `hf whoami` | Verifies the account online (an expired/revoked token shows up here). 1 if not signed in | `{kind, user, expires_at, scope}` |

### Text to speech

No instance needed: the commands run inside this process and take no lock. Audio is produced with `sherpa-onnx-offline-tts` (the managed sherpa-onnx runtime); voice models are folders in the model directories (Kokoro, Piper/VITS) and appear in the library with the `tts` type. The same job is `POST /v1/audio/speech` on the gateway.

| Command | What it does | `--json` |
|---|---|---|
| `speak [TEXT] [-m MODEL] [--voice VOICE] [-s SPEED] [-f wav\|pcm\|mp3\|opus\|aac\|flac] [-o FILE] [--runtime ID] [--threads N]` | Reads the text and writes the audio to a file (`speech.<format>` if there is no `-o`; `-o -` writes the audio data to stdout and refuses to write to a terminal). The text is an argument; if it is `-` or there is no argument (a pipe), it is read from stdin. Without `-m`, the first installed model; it can be a model name, a folder name, or an OpenAI name such as `tts-1`. `--voice` is the name of a model voice (`af_heart`, `fettah`), an index number, or an OpenAI name (`alloy` → Kokoro `af_alloy`); if not given, the model's default voice (Kokoro `af_heart`). Speed 0.25-4.0. At most 4096 characters of text. `wav` and `pcm` always work, the other formats only if ffmpeg is present. 3 if there is no model (`tts.no_model`), 2 for an invalid format/speed/text, `runtime.family_not_installed` if there is no runtime | `{file, model, voice, format, sample_rate, duration_secs, bytes, elapsed_ms}` (`--json` cannot be used with `-o -`: exit 2) |
| `tts ls` | Installed voice models: family, languages, quant, size, voices | `[{name, id, family, languages, quantization, sample_rate, size_bytes, voices}]` |
| `tts catalog` | Recommended downloads (the official sherpa-onnx release): Kokoro multilingual v1.0 int8 (Apache-2.0), the Turkish Piper voices `fettah` and `fahrettin` (CC0); size, license, whether installed | `[CatalogEntry + installed]` |
| `tts install <id> [-y]` | Downloads and extracts a catalog model with SHA-256 verification (`<models>/tts/<id>/`); progress goes to stderr | the installed model |
| `tts rm <model> [-y]` | Deletes a voice model folder (only folders inside the model directories that are recognized as voice models) | `{removed}` |

The runtime is installed once: `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx` (in the GUI, Settings > Runtime > "Fill sherpa-onnx Repository", or "Install Engine" on the Speech tab).

```sh
harness tts install vits-piper-tr_TR-fettah-medium-int8
harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx -y
harness speak "Merhaba dünya" --model fettah -o merhaba.wav
echo "Read this aloud." | harness speak - -f mp3 -o out.mp3
```

### Speech recognition (sherpa-onnx)

No instance needed: the commands run inside this process and take no lock. Text is read with `sherpa-onnx-vad-with-offline-asr` (the same sherpa-onnx runtime as text to speech); models are folders in the model directories (NVIDIA Parakeet, SenseVoice, Omnilingual...) and appear in the library as "Speech recognition (sherpa-onnx)". Whisper (ggml `.bin`) models are not used with these commands but through the gateway (`POST /v1/audio/transcriptions`); the same endpoint also runs installed sherpa models by name.

| Command | What it does | `--json` |
|---|---|---|
| `transcribe FILE [-m MODEL] [-l LANG] [-f text\|json\|verbose_json\|srt\|vtt] [-o FILE] [--runtime ID] [--threads N]` | Converts an audio file to text. WAV (16-bit mono) is read as is; other formats (MP3, M4A, WebM...) need ffmpeg. A long recording is split at pauses, and each segment's time is produced. Without `-m`, the first installed model; it can be a model name, a folder name, a file path, or a unique part of the name (`parakeet`). `-l` is effective only for SenseVoice (`zh en ja ko yue`); other models detect the language themselves. Output goes to stdout (to a file with `-o`); progress and the summary go to stderr. 1 if there is no model (`stt.no_model`), `runtime.family_not_installed` if there is no runtime | the full result if there is no `-o` (`verbose_json`: `text`, `duration`, `segments`); with `-o`, `{file, model, duration_secs, segments, elapsed_ms}` |
| `asr ls` | Installed recognition models: family, languages (from the catalog), quant, size | `[{name, id, family, languages, quantization, size_bytes}]` |
| `asr catalog` | Recommended downloads (the official sherpa-onnx `asr-models` release): Parakeet TDT 0.6B v3 int8 (25 European languages, CC-BY-4.0), SenseVoice Small int8 (zh/en/ja/ko/yue), Omnilingual ASR 300M v2 int8 (1600 languages, Turkish included, Apache-2.0); size, license, whether installed | `[CatalogEntry + installed]` |
| `asr install <id> [-y]` | Downloads and extracts a catalog model with SHA-256 verification (`<models>/asr/<id>/`); Silero VAD (644 KB) comes along; progress goes to stderr | the installed model |
| `asr rm <model> [-y]` | Deletes a model folder (only folders inside the model directories that are recognized as recognition models) | `{removed}` |

The runtime is shared with text to speech and installed once: `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`. The release has no Turkish-specific model: for Turkish, use Omnilingual (no punctuation, lowercase) or Whisper (a whisper.cpp model from Model Hub).

```sh
harness asr install sherpa-onnx-omnilingual-asr-1600-languages-300M-ctc-v2-int8-2026-02-05
harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx -y
harness transcribe meeting.wav -m omnilingual -f srt -o meeting.srt
harness transcribe speech.mp3 -m parakeet
```

### Lemonade (Ryzen AI NPU and its own llama.cpp builds)

No instance needed: the commands run inside this process. Lemonade is an optional engine ([ARCHITECTURE.md](ARCHITECTURE.md) 14.5): it runs models on Ryzen AI NPUs with FastFlowLM and offers its own ROCm / Vulkan llama.cpp builds. Once installed, downloaded models appear on the gateway under their Lemonade names; the gateway starts `lemond` itself when needed. The `pull`, `rm`, `backend`, and `models --refresh` commands start their own `lemond` for the duration of the command and stop it at the end (separate from the `lemond` of an open Harness window). Commands that make changes ask for confirmation (`-y`).

| Command | What it does | `--json` |
|---|---|---|
| `lemonade status` | Installed versions, the llama.cpp backend setting, NPU state (detected / no driver / not detected), whether `lemond` is running in this process, the backends of the last run | `LemonadeStatus` (`GET /lemonade`) |
| `lemonade install [--tag T]` | Downloads the latest stable Lemonade release (GitHub `lemonade-sdk/lemonade`, `lemonade-embeddable-*`), verifies SHA-256, tries `lemond --version`, and registers it as a `lemonade` family runtime. A new version is a new entry; the old one is the rollback path | the added entry |
| `lemonade uninstall <id>` | Deletes an installed version (`lemonade.in_use` if it is running) | `{removed}` |
| `lemonade config [--backend auto\|vulkan\|rocm\|cpu] [--runtime ID\|default]` | Sets the llama.cpp backend and the version to run (pinning the old one for rollback) | `LemonadeSettings` (`PUT /lemonade/settings`) |
| `lemonade models [--refresh] [--downloaded]` | Lemonade's model registry. Without `--refresh`, the list from the last run; `--refresh` starts `lemond` and reads the current list | `{live, updated_at, models}` (`GET /lemonade/models`) |
| `lemonade pull <model>` | Has Lemonade download a model by registry name (`Qwen3-0.6B-GGUF`) or `owner/repo[:variant]`; progress goes to stderr | `{pulled}` (`POST /lemonade/models/pull`) |
| `lemonade rm <model>` | Deletes a downloaded model | `{removed}` |
| `lemonade backend <recipe> <backend>` | Installs a backend (`llamacpp rocm`, `llamacpp vulkan`, `flm npu`...). `flm npu` cannot be installed without an NPU | `{installed}` (`POST /lemonade/backends/install`) |

```sh
harness lemonade install
harness lemonade config --backend vulkan
harness lemonade pull Qwen3-0.6B-GGUF
# with harness serve running, the gateway serves the model under its Lemonade name:
curl http://127.0.0.1:11435/v1/chat/completions -H "content-type: application/json" \
  -d '{"model":"Qwen3-0.6B-GGUF","messages":[{"role":"user","content":"Hello"}]}'
```

### Converting safetensors to GGUF

No running instance needed; the commands run inside this process. It downloads a safetensors LLM repository from Hugging Face (text generation or vision chat, on architectures llama.cpp recognizes), converts it to GGUF with llama.cpp's official `convert_hf_to_gguf.py` script, quantizes it with the managed runtime's `llama-quantize`, and puts it under `<models>/<owner>/<repo>-GGUF/` (the library sees it like any download). Python runs only at conversion time, in a temporary environment that Harness itself sets up on first use (uv + Python + torch CPU, ~700 MB; for details and licenses see [ARCHITECTURE.md](ARCHITECTURE.md) 14.5). A managed llama.cpp runtime must be installed; for gated repositories, use `harness hf login`.

| Command | What it does | `--json` |
|---|---|---|
| `convert <owner/repo> [--quant TYPE] [--keep-original] [--plan] [-y]` | First prints the plan (architecture, parameters, download and result size, required and free disk, whether the environment will be installed), asks for confirmation, then runs; stage and progress go to stderr. `--quant`: `Q4_K_M` (default), `Q5_K_M`, `Q6_K`, `Q8_0`, `F16`, `BF16`. `--keep-original` leaves the downloaded safetensors under `<models>/<owner>/<repo>/`. `--plan` only shows the plan and exits. Ctrl+C cancels the job (what was downloaded stays, and the same command resumes). Exits with 1 on an unsupported architecture, a non-LLM repository, insufficient disk, a gated repository, or a converter error | `--plan`: `ConvertPlan` (+ `quant`, `estimate`); otherwise the finished `ConvertJob` (`output`, `mmproj`, `warning`) |
| `convert env status` | The conversion environment: is it installed, for which llama.cpp tag, size, location | `EnvStatus` |
| `convert env install [-y]` | Installs the environment now (otherwise the first conversion installs it) | `EnvStatus` |
| `convert env rm [-y]` | Deletes the environment (~700 MB comes back; the next conversion installs it again) | `{removed}` |

Control API: `GET /convert/plan?repo=owner/repo`, `POST /convert` (`{repo, quant?, keep_original?}`), `GET /convert`, `GET /convert/{id}`, `POST /convert/{id}/cancel`, `DELETE /convert/{id}`, `GET|POST|DELETE /convert/env`. `POST /convert`, cancel, and delete require the `downloads:write` scope; installing/deleting the environment requires `admin`; reads require `read`. Job changes arrive on the event stream as `convert` and `convert_removed`.

```sh
harness convert HuggingFaceTB/SmolLM2-135M-Instruct --plan
harness convert Qwen/Qwen3-0.6B --quant Q5_K_M -y
harness convert env status
```

### Servers

| Command | What it does | `--json` |
|---|---|---|
| `ps [-a]` | Running servers; `-a` also shows stopped ones and profiles | `ServerView[]` |
| `load <model> [--server ID] [--no-wait] [--timeout S]` | Loads the model and waits until it is ready. `<model>` can be a definition alias, a library model, or a `.gguf` path; if it has no definition, one is created from the file name (written to stderr). If a stopped instance last served the same definition, it is reused; otherwise a new instance is opened on the next free port | `{server_id, definition, server}` |
| `retry <id> [--option O] [--set ID=V]... [--unload ID]... [--save] [--no-wait] [--timeout S]` | Restarts a server that errored. Without options: lists the error and the recovery options (`GET /servers/{id}/recovery`). `--option` applies an option (for example `lower_context`, `enable_fit`, `kv_cache_quant`, `unload_server:<id>`), and `--set` applies individual catalog values for this launch. `--unload` first stops another server. `--save` writes the values to the definition (otherwise they are temporary) | Without options: `RecoveryPlan {server_id, failure, options: [{id, kind, label_key, params, overrides, unload, recommended}], free_vram_bytes, estimated_need_bytes}`; when launching, `ServerView` |
| `unload <model> \| --server ID` | Stops all servers serving the definition (or one) | `{stopped: [...]}` |
| `stop <id>... \| --all` | Stops servers | `{stopped: [...]}` |
| `logs <id> [-f] [-n N]` | The last N lines; `-f` follows new lines until Ctrl+C | `LogsResponse`; `-f --json` one JSON line per batch |
| `lora list <id>` / `lora set <id> <adapter> <scale>` | Lists the running server's LoRA adapters (id, scale, path); `set` changes the scale of a single adapter without restarting the server, and the others stay the same (0 turns it off). If the server is not running, `lora.server_not_running` (exit 4). Ids follow the `--lora` order; to set the scale to 0 at startup, use `--lora-init-without-apply` | `GET`/`POST /servers/{id}/lora` (`{adapters: [{id, path, scale}]}`; POST body `{adapters: [{id, scale}]}`, scope `servers:control`) |
| `router models <server>` | Lists the models and states (`loaded`, `loading`, `unloaded`, `failed`) of a router server (`--models-dir`, many models in one process) (`GET /servers/{id}/router/models`). If the server is not running or is not a router, exit 4 (`router.not_running` / `router.not_router`) | `{models: [{id, state, source, path, context_length, exit_code}]}` |
| `router load <server> <model> [--no-wait] [--timeout S]` | Loads one of the router's models and waits until it is `loaded` (default 300 s; an error if it cannot be loaded, with the exit code in the message). If the router exceeds `--models-max`, it unloads the oldest loaded model itself | `RouterModel` |
| `router unload <server> <model>` | Unloads one of the router's models | `RouterModel` |
| `shutdown` | Cleanly shuts down a `harness serve` instance (`POST /shutdown`, `admin`). The GUI does not accept it (4) | `{shutdown: true}` |

For `load`, `run`, `profile run`, and `retry`, the `--timeout` default is 0: it waits until the server's own load timeout (`load_timeout_secs`, default 300 s; `load_timeout` in the definition) finishes the launch. If launch fails, the error carries the `failure` object (`code`, `kind`, `message_key`, `params`, `phase`, `exit_code`, `evidence`) and the last lines of the log (`log_tail`).

### Run and chat

| Command | What it does | `--json` |
|---|---|---|
| `run <model> [prompt...]` | Loads the model if it is not running, sends the prompt, and streams the answer to stdout. If there is no prompt, stdin is read; if stdin is a terminal, `chat` opens. Reasoning text is written dimmed to stderr; a speed summary at the end goes to stderr. If this command loaded it, the server keeps running; `--unload` stops it | `{model, server_id, endpoint, loaded, content, reasoning_content, finish_reason, interrupted, usage, timings}` |
| `chat <model>` | Interactive chat; history is kept. `/bye` (or Ctrl+D) exits, `/clear` resets. stdin can be a file/pipe (each line is a message) | one JSON line per answer |
| `image [prompt...] --model <image model>` | Generates one or more images with an image model through the gateway's `/v1/images/generations` endpoint and writes them to files. If there is no prompt, stdin is read. Options: `-o/--output` (default `image.png`; with several images `name-1.png`, `name-2.png`), `--size 512x512` (a multiple of 8; the model's size if not given), `--negative`, `--steps`, `--cfg-scale`, `--seed` (-1 random), `--sampler`, `-n/--count`, `--format png\|jpeg\|webp`, `--timeout` (model loading included, default 1800 s). The model loads by itself on the first request. If the gateway is not running, `cli.not_available` | `{model, files}` |

Common options: `--system`, `--max-tokens`, `--temperature`, `--api-key`, `--timeout`. The request goes to `/v1/chat/completions` as a stream:

- If the instance's gateway is running, through the **managed endpoint** (`endpoint` is the gateway address; the request is marked with `x-harness-client: harness-cli`). On a gateway that needs a token, `<data>/gateway-token` is used for a local owner and `HARNESS_TOKEN` (named, with the `inference` scope) for a remote instance. On a remote instance, a gateway bound only to loopback cannot be used.
- Otherwise directly to the model's llama-server endpoint. On a remote instance, the endpoint's `127.0.0.1` address is replaced with the API's host.

### Headless

| Command | What it does |
|---|---|
| `serve [--listen HOST] [--port N] [--no-restore] [--gateway \| --no-gateway] [--gateway-port N]` | Becomes the owner of the data directory (4 if the lock cannot be taken), loads the download queue, starts the telemetry sampler, opens the Control API (the address/port in settings; always on here; if the port in settings is held by another program, it falls back to a spare port and prints `warning:`, and if a port given with `--port` is in use, it exits with 4), and restores profiles that start automatically and the last session. It shuts down cleanly on Ctrl+C, SIGTERM/SIGHUP (Unix), console close/logoff (Windows), or `harness shutdown`: all llama-servers stop, the API closes, and `instance.json` and `api-token` are deleted. The gateway (a single endpoint, JIT loading, a queue; default `127.0.0.1:11435`) starts if it is on in settings or `--gateway` is given; `--no-gateway` keeps it off, and `--gateway-port` changes the port for this run (if the gateway port in settings is taken, it falls back to a spare port and prints a `warning:` line; a port given with `--gateway-port` does not fall back). If it cannot start, a warning is printed and `serve` continues. If the gateway's Ollama port (if on in settings) cannot be taken (for example, real Ollama is running), a `warning:` line is printed. `--json`: when ready, `{"event": "ready", pid, kind, base_url, token_path, data_dir, version, gateway: {running, base_url, require_token, error, ollama_port, ollama_warning, port_fallback}}`, and when shutting down, a `{"event": "stopped", reason}` line |
| `service install\|uninstall [--name N]` | Starts `harness serve` at login. Linux: `~/.config/systemd/user/harness.service` + `systemctl --user enable --now`. macOS: `~/Library/LaunchAgents/com.harnessstudio.harness.plist` + `launchctl bootstrap gui/<uid>` (log: `<data>/logs/harness-serve.log`). Windows: `HarnessStudio\harness-serve` in Task Scheduler, a logon trigger for the current user, lowest privilege, no time limit, 3 retries on failure. The task launches `harness-hidden.exe` next to `harness.exe` (a launcher that opens no console window; it runs `harness serve` without a window and closes it when the task ends). If the launcher is missing, the task runs `harness.exe` and a console window stays open. If `--data-dir` is given, it is passed to the service as well |
| `service print [--platform systemd\|launchd\|windows]` | Prints the unit / plist / task XML without installing |

Windows note: because the task runs a console application, a console window appears at logon. "End" on the task kills the process; llama-servers close with it through the Job Object (use `harness shutdown` for a clean shutdown).

### Profiles, benchmark, tokens

The old name of the `token` command, `api-token`, also works.

| Command | What it does | `--json` |
|---|---|---|
| `profile ls` | Saved server profiles | `ServerView[]` with profiles |
| `profile show <id>` | Profile detail | `ServerView` |
| `profile run <id> [--no-wait] [--timeout S]` | Starts the profile and waits until ready | `ServerView` |
| `profile export <id> [-o FILE]` | Profile and model definition: `{"format": "harness-profile", "version": 1, profile, definition}`. The key reference and the binary path are not written. The document is written to stdout as JSON even without `--json` | the document or `{id, file}` |
| `profile import <FILE\|->` | If a local definition with the same alias exists, it is used (a warning if the model file differs); otherwise the definition is created; then the profile is added. 4 if a profile with the same id exists. If the profile is rejected, the newly created definition is deleted again | the created `ServerView` |
| `import modelfile <FILE\|-> [--alias NAME] [--dry-run]` | Imports an Ollama Modelfile as a model definition + inference preset (relative paths start from the file's folder). `--dry-run` only shows what would be created (mapped values, warnings) and does not save. If no alias is given, it is derived from `FROM`; an error if it is in use (give another name with `--alias`). `MESSAGE` examples are added to the preset's system prompt as "Example conversation" (the `messages_as_examples` warning reports this). Warnings (Go `TEMPLATE`, an unmapped parameter, the `MESSAGE` note) are written to stderr | `{definition, warnings}`; `--dry-run`: `{import, mapped, warnings, from}` |
| `import modelfile --ollama <name[:tag]> [--alias NAME] [--dry-run]` | Imports a model installed in Ollama: the GGUF file in Ollama's folder (`OLLAMA_MODELS`, `~/.ollama/models`) is used in place, read-only; the model's stored parameters and system prompt are inherited | same |
| `benchmark run <model> [--n-prompt 512] [--n-gen 128] [--ngl ...] [--repetitions N] [--threads N]` | Measures with the `llama-bench` next to llama-server, waits until done, and Ctrl+C cancels. Lists are comma-separated (a sweep) | `BenchmarkView` |
| `benchmark ls` / `benchmark show <id>` | Saved runs | `GET /benchmarks`, `GET /benchmarks/{id}` |
| `token create <name> --scope read[,inference,...] [--model PATTERN]... [--endpoint GROUP]... [--rpm N] [--tpm N] [--concurrency N]` | A named token; the value is shown once. Scopes: `read`, `inference`, `models:write`, `downloads:write`, `servers:write`, `servers:control`, `conversations` (reads/exports/compacts chats; does not include `read`), `tools` (runs Harness tools; does not include `read`), `admin`. `--model` allowed alias patterns (`qwen3-*`; `*` = all), `--endpoint` gateway groups (`chat`, `embeddings`, `rerank`, `models`, `messages`, `ollama`, `metrics`; `all` = all); both can be repeated or comma-separated. Limits: requests per minute, tokens per minute (prompt + response), and concurrent requests | `CreatedToken` |
| `token ls` | Tokens, scopes, MODELS / ENDPOINTS / LIMITS columns (never the values) | `TokenInfo[]` |
| `token show <id>` | A token's scopes, allowed models, endpoints, and limits | `TokenInfo` (`GET /tokens/{id}`) |
| `token set <id> [--name NAME] [--scope ...] [--model ...] [--endpoint ...] [--rpm N] [--tpm N] [--concurrency N]` | Changes only the fields given; the token value stays the same. In limits, `0` = no limit | `TokenInfo` (`PATCH /tokens/{id}`) |
| `token rm <id> [-y]` | Revoke (takes effect immediately) | `{revoked}` |
| `otel status` | OpenTelemetry settings (header names only), delivery counters, and the protocols in the build (`status.grpc`; in the default build only HTTP/protobuf) | `{settings, status}` (`GET /otel`) |
| `otel test [--endpoint URL] [--protocol http\|grpc]` | Sends a real test span to the saved (or given) collector; exit 1 if the collector rejects it. `grpc` only in an instance built with `otel-grpc`; in others exit 2 (`otel.grpc_not_built`) | `{ok, url, protocol, elapsed_ms, trace_id, error}` |
| `completion <bash\|zsh\|fish\|powershell\|elvish>` | A shell completion script | |

### Runtimes

Named llama-server builds other than the official llama.cpp (for example the PrismML fork) are registered; a model definition (`load.runtime`) or a server profile (`serve_overrides.runtime`) selects one of them. If none is selected, the official runtime in Settings > Runtime is used. `--family` can be `llama.cpp`, `whisper.cpp`, or `stable-diffusion.cpp` for image generation (selected automatically if the GitHub repository name is one of these: `runtime add --github leejet/stable-diffusion.cpp`; if `--backend` is not given, the build suited to the machine: Vulkan on AMD, CUDA on NVIDIA, CPU otherwise). `official` and (if a llama-server was found on PATH/WinGet) `system` are virtual entries and cannot be added or deleted. Commands that make changes require the `admin` scope.

| Command | What it does | `--json` |
|---|---|---|
| `runtime ls` | The official runtime (its real source: managed / custom / found on the system; a warning if it differs from the configured one; a staging build that has not been activated), the system llama-server, and registered runtimes: version, backend, trust level, flag count, unsupported catalog parameters, number of quant types | `RuntimeView[]` (`GET /runtimes`) |
| `runtime add --github OWNER/REPO [--family F] [--tag T] [--asset A] [--backend B] [--allow-unverified] [--name N] [--id ID]` | Installs from a GitHub release. `--family` is the rule family that selects the archive: `llama.cpp` (default), `whisper.cpp`, `sherpa-onnx`, `lemonade`. First the archive name, size, SHA-256 digest, and download address are shown and confirmation is requested; download progress is written. If `--backend` is not given, the hardware recommendation is used (on CUDA the `cudart` archive is added too). An archive without a digest is installed only with `--allow-unverified`. Installation goes under `<data>/bin/runtimes/<id>/` and is tested with `--version` | the added `RuntimeEntry` |
| `runtime add --local DIR [--family F] [--copy] [--name N] [--id ID]` | Registers a folder that contains llama-server (or the binary of the `--family` family) (used in place; `--copy` copies it into the data directory). A SHA-256 manifest of the files is produced. `--family mistral.rs` can only be added this way (there is no prebuilt download): a `mistralrs(.exe)` folder you built yourself, for example `harness runtime add --local C:\src\mistral.rs\target\release --family mistral.rs`; adding runs `mistralrs --version` and records the version | the added `RuntimeEntry` |
| `runtime rm <id> [--force] [-y]` | Deletes the entry; downloaded/copied files are deleted, and a local folder is not touched. If a running server uses it, always, and if a definition/profile selects it, without `--force`, `runtime.in_use` (exit 4). A definition/profile that keeps selecting a runtime deleted with `--force` gets `runtime.not_found` when it is launched | `{removed}` |
| `runtime verify <id> [--accept-changes]` | Re-hashes the files and runs `--version`. If there is a change, exit 4 (`runtime.changed`) and the changed/missing/new files are listed. `--accept-changes` (local only) makes the current files the new baseline | `VerifyReport {ok, accepted, changed, missing, added, version, error}` |
| `runtime suggest <model>` | Looks at the model's GGUF tensor types and says which runtime can load it (for example Bonsai `PQ2_0` → the PrismML fork; if not registered, the repository to add) | `RuntimeAdvice` (`GET /runtimes/advice`) |
| `runtime managed status` | The managed official build in Settings > Runtime: the installed and previous version, the backend that updates preserve (recorded and detected from files), the build in the backup slot | `ManagedRuntimeStatus` (`GET /runtime/managed`) |
| `runtime managed install [--backend B] [--allow-unverified] [-y]` | Downloads the newest official build, verifies it (SHA-256, `--version`, a device check for a GPU build), and activates it; the old build moves to the backup. With `--backend`, exactly that backend (a switch); without it, the installed build's backend (an update; it does not fall back to another backend); if none is installed, the hardware recommendation. If a server is running, `runtime.stop_servers_first` (exit 4) | `ManagedInstall` (`POST /runtime/managed/install`) |
| `runtime managed rollback [-y]` | Returns to the backed-up build; the installed build moves to the backup. If there is no build in the backup, `runtime.no_backup` (exit 4) | `ManagedRuntimeStatus` |

If, at launch, the selected runtime does not explicitly support the model's tensor type (types are read from that build's `llama-quantize --help` list), the server is not started: `runtime.quant_unsupported` (exit 4). If support is unknown, the launch is not blocked.

### Conversations

Playground conversations (`state.db`); require the `conversations` scope (the `harness` session token is fully privileged). Alias: `chats`. It connects to the running instance; for compaction the gateway must be on (the summary request goes through the gateway).

| Command | What it does | `--json` |
|---|---|---|
| `conversations ls [QUERY] [--folder ID] [--pinned] [--favorites] [-n N]` | Lists conversations newest first; the query searches titles and message content (FTS) | `[ConversationSummary]` |
| `conversations show <id>` | The conversation's current path: messages, markers (compaction) | `{conversation, messages}` |
| `conversations export <id> [-f json\|markdown] [-o FILE]` | `harness.conversation.v1` JSON (the file the app imports) or Markdown; stdout if there is no `-o` | the JSON document |
| `conversations docs <id>` | Metadata of the documents attached to the conversation (name, type, size, state); no content | `[RagDocument]` |
| `conversations compact <id> [-m ALIAS] [--n-ctx N] [--keep-turns N] [--keep-tool-results] [--summary-tokens N] [-i INSTRUCTION]` | Replaces old turns with a summary written by the model (no message is deleted; a marker node is added) | `{marker_id, marker, ...}` |
| `conversations undo <id> <marker> [--redo]` | Undoes a compaction (requests send the full history) or reapplies it | `{undone}` |

### Analytics

The record of requests that pass through the gateway (the managed endpoint) (`analytics.db`). Requests that go directly to the llama-server port cannot be seen. Request content is stored only if content recording is on in the model definition.

| Command | What it does | `--json` |
|---|---|---|
| `analytics [--range 24h\|7d\|30d\|90d\|all]` | Per model: requests, tokens, average generation t/s, TTFT and latency P50/P95, VRAM peak, error and OOM rate, last use; the client breakdown; the last llama-bench result vs. actual speed. The default is `7d`. Days older than the raw-row retention period come from daily totals (no percentiles). An invalid range: `api.invalid_range` (exit 2) | `GET /analytics/summary` (`AnalyticsSummary`) |
| `requests [--model A] [--client C] [--status ok\|error\|oom\|cancelled] [--tool-issue] [--tool-category runtime\|context\|model\|client] [-n N]` | The last N requests (default 20): time, model, client, endpoint, status (`tool:<category>/<code>` is added if there is a tool diagnosis), tokens, TTFT, total duration, generation t/s. `--tool-issue` returns only requests with a tool-call finding ([TOOL-DIAGNOSIS.md](TOOL-DIAGNOSIS.md)) | `GET /analytics/requests` (`RequestPage`) |
| `requests <id>` | A single request: all fields, the timeline (load → queue → prompt → generation → other), error detail, tool diagnosis (category, code, evidence); the content, if it was recorded, is in the `--json` output | `GET /analytics/requests/{id}` (`RequestDetail`) |

### Tools

Harness's own tool engine (`harness_core::tools`): `read_file`, `file_glob_search`, `grep_search`, `get_info` (restricted), `write_file`, `edit_file`, `exec_shell_command` (dangerous), with the same names and schemas as llama-server's seven tools. The policy (`settings.json` → `tools`: `off`, `ask_every_time`, `allow_trusted`, `allow_all`; default `ask_every_time`) decides whether each call runs, is asked about, or is refused. File tools cannot leave the working directory.

| Command | What it does | `--json` |
|---|---|---|
| `tools list` (`tools ls`) | Tools: name, class (`safe` / `restricted` / `dangerous`), source, the policy's current decision (`run`, `ask`, `refuse`), description; the policy and trusted tools on top | `GET /tools` (`ToolListing`) |
| `tools mcp [--retry]` | Harness's own MCP client: for every configured server, the state (`idle`, `connecting`, `connected`, `failed` + error, `disabled`), transport, negotiated MCP version, and tool count. It starts servers on the first call; `--retry` retries those that did not start. The `AUTH` column shows the OAuth state (`required`: authorization needed, `authorized`) | `GET /tools/mcp` |
| `tools mcp-resources <server> [--read <uri>]` | The resources and resource templates of an MCP server; `--read` prints a resource's content | `GET /tools/mcp/{server}/resources`, `/resource?uri=` |
| `tools mcp-authorize <server>` | Authorizes a remote MCP server with OAuth: the browser opens on the machine where `harness` runs, approval is awaited, and the token is stored in the keychain (the CLI client times out in 3 minutes) | `POST /tools/mcp/{server}/authorize` |
| `tools mcp-logout <server>` | Deletes the server's OAuth session | `POST /tools/mcp/{server}/logout` |
| `tools run <name> [--args '<json>'] [--workdir DIR] [--timeout SEC]` | Runs a tool; the output is the tool's text. Without `--workdir`, the current directory. If the decision is `ask`, a y/N question on the terminal; with no terminal, `--yes` (`-y`) is needed (exit 2, `cli.confirmation_required`). If the policy is `off`, `tools.disabled` even with `--yes` (exit 1). If the tool runs and errors (outside the path, an unmatched edit, a timeout), exit 1 with the message on stderr; `--json` also writes the result to stdout | `POST /tools/{name}/run` (`RunToolResponse`) |

The same surface in the Control API: `GET /tools`, `POST /tools/{name}/run`, `GET /tools/policy`, `PUT /tools/policy`, `GET /tools/log`. Running, changing the policy, and the log require the `admin` scope. Because the API cannot ask the user, a call the policy says to "ask" about returns `403 tools.approval_required`; a client that does ask (the CLI) sends `"approved": true` in the body after getting a yes from the user.

### MCP

| Command | What it does |
|---|---|
| `mcp [--scope SCOPES]` | The Harness MCP server, over stdin/stdout (one JSON-RPC message per line). Clients such as Claude Code, Codex, OpenCode, and Cursor start this command themselves. Tools go through the running instance's Control API; the instance is rediscovered on every call. If there is no instance, tools return `cli.no_instance`; with `--headless`, a temporary instance is opened for the life of the process (it closes with its servers when stdin closes). `--scope` (default `read,servers:write,tools`) determines which tools are listed and callable; `models:write` opens downloading and deletion; `tools` shows Harness's built-in tools only if "Offer to MCP Clients" is on in Settings > Tools. Only MCP messages are written to stdout. Tools and protocol: [MCP.md](MCP.md). An invalid scope: exit 2 |

```sh
claude mcp add -s user harness -- harness mcp
codex mcp add harness -- harness mcp --scope read,servers:write,models:write
```

### Launching a coding agent

| Command | What it does | `--json` |
|---|---|---|
| `launch <claude\|codex\|opencode\|aider\|goose> [--model M] [--prepare \| --no-prepare] [--direct] [--api-key K] [--timeout S] [--dry-run] [-- ARGS]` | Starts the tool connected to Harness; only the tool's process environment and temporary files are used, and global configuration such as `~/.claude` and `~/.codex` is not written. Without `--model`, the list of definitions is asked on a terminal (with no terminal, the only running model, and if there is none, 2). If the context is below the tool's minimum or unknown, "Prepare Server" is asked: the definition is written with the largest context the model supports and that fits this machine's GPU (within the tool's limits), Flash Attention, `f16` KV (if it does not fit, in order K `q8_0`, `q8_0`/`q8_0`, K `q8_0` + V `q4_0`; see ARCHITECTURE "KV cache precision"), and 1 slot, and running instances are stopped; the choice, the reason, and the alternatives are printed first. When it does not fit entirely on the GPU (the minimum context), `--prepare` is not enough: partial CPU spill is accepted only with explicit confirmation/`--yes`, and if no option fits, 5 (`cli.launch_context`); on a remote instance, since the hardware is different, a fixed recommendation (65,536, q8_0) is used; with no terminal, `--prepare` or `--no-prepare` is required (otherwise `cli.launch_context`, 2). If the gateway is running, the managed endpoint and the gateway client token are used, and if the model is not loaded a one-token warm-up request is sent (OOM: 5); with `--direct` or if there is no gateway, the model is loaded with `/load`. Arguments after `--` go to the tool (in Claude for example `-p "..."`, in Codex `exec ...`). Ctrl+C is left to the tool; when the tool exits, the temporary folder is deleted. The exit code is the tool's; 3 if the tool is not on PATH (`cli.launch_not_installed`). Details: [INTEGRATIONS.md](INTEGRATIONS.md) | `--dry-run`: `{tool, model, definition_id, endpoint: {kind, base_url, token: "***"\|null}, context: {per_request, min, recommended, check}, prepare: {definition_load, plan}, load, program_path, plan: {program, args, env, env_remove, files, ...}}`; token values are `***` |

```sh
harness launch claude --model Qwen3-4B-Instruct-2507-Q4_K_M
harness launch codex -m qwen3-coder --prepare -- exec "Summarize the README"
harness launch opencode --dry-run --json
```

## Examples

```sh
# A headless instance, used from another terminal
harness serve
harness pull unsloth/Qwen3-4B-Instruct-2507-GGUF:Q4_K_M
harness run Qwen3-4B-Instruct-2507-Q4_K_M "Say hello."
harness ps --json | jq '.[].endpoint'
harness shutdown

# One-off, with no instance
harness --headless run ./model.gguf "2+2?" --json

# Out of memory (exit 5): see the options, try the recommended one
harness load qwen14b --json || harness retry qwen14b
harness retry qwen14b --option lower_context
harness retry qwen14b --set context_size=16384 --set cache_type_k=q8_0 --save

# Remote instance (named token)
HARNESS_HOST=192.168.1.20:11500 HARNESS_TOKEN=hs_... harness ps
```

## Tests

- Unit tests: `cargo test -p harness-cli` (error mapping, text equality with the GUI translation, service unit/plist/task contents, argument parsing).
- Integration (`tests/cli.rs`): the real `harness` binary is run with temporary data directories; `--json` schemas and exit codes, the temporary instance releasing the lock, rejection of a second owner (4), a busy port (4), remote token scopes, the `serve` + `shutdown` lifecycle.
- `import modelfile` (`tests/cli.rs`): `--dry-run` saves nothing, a real import creates the definition and preset, an alias conflict and a conflict between `FILE` and `--ollama` give errors.
- `transcribe` / `asr` (`src/commands/asr.rs`): argument parsing, output formats, `need()` = no instance needed, the English text of keyed errors. Real recognition is in `asr/e2e.rs` and the gateway end-to-end test ([ARCHITECTURE.md](ARCHITECTURE.md)).
- `speak` / `tts` (`src/commands/tts.rs`): argument parsing, stdin/argument text, output target and format validation, `need()` = no instance needed, the English text of keyed errors. Real audio generation is in the gateway end-to-end test ([ARCHITECTURE.md](ARCHITECTURE.md)).
- MCP (`tests/mcp.rs`): the real `harness mcp` binary with line-by-line JSON-RPC; handshake, scopes, tool errors, modern/legacy era. `#[ignore]` end to end: `load_model` → `get_endpoint` → a real chat → `unload_model` → benchmark; configuration checks with the real `claude mcp list` and `codex mcp get` (a temporary `CLAUDE_CONFIG_DIR` / `CODEX_HOME`).
- `launch` (`tests/launch.rs`, `#[ignore]`, ports 19120+): the `--dry-run` plan of five tools with `serve --gateway` (token hidden), connection tests (`/v1/messages`, `/v1/responses`, chat; a tool call), the real Claude Code with `HS_TEST_LAUNCH_CLAUDE=1` (a temporary `CLAUDE_CONFIG_DIR`, `--prepare`, `-p`), the real Codex with `HS_TEST_LAUNCH_CODEX=1` (a temporary `CODEX_HOME`, `exec`). Usage errors are in `tests/cli.rs`.
- End to end (`tests/e2e.rs`, `#[ignore]`): `HS_LLAMA_SERVER=<llama-server> HS_TEST_MODEL=<gguf> cargo test -p harness-cli --test e2e -- --ignored --test-threads=1`. `serve` → `run` (a definition from a file path) → reuse by alias → `chat` (two turns, history) → `logs` → `benchmark run` → `unload` → `load` → `shutdown` (ports close); nothing stays open after `--headless run`. With `HS_TEST_OOM=1`, exit 5 and `servers.out_of_memory` (stage `kv_cache`) are verified on a real VRAM allocation failure; `e2e_oom_recovery_retry` verifies that the server really opens with `harness retry --option lower_context` and that it starts without `-ngl` with `enable_fit`; `e2e_crash_auto_restart` verifies that a crashed llama-server is restarted with exponential backoff and that `stop` cancels the attempt while waiting. `e2e_serve_gateway_run_uses_managed_endpoint` verifies the `serve --gateway` ready line, the gateway information in `status`, and that `run` goes through the gateway (ports 18800+). `e2e_serve_gateway_request_log_and_analytics` verifies that the gateway request of `run` is written to `analytics.db` and read back with `requests`, `requests <id>`, and `analytics` (ports 19010+).
