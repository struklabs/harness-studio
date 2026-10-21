//! Command-line definition (clap derive).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser, Debug)]
#[command(
    name = "harness",
    version,
    about = "Harness Studio from the command line: models, servers, profiles, benchmarks and a headless mode.",
    long_about = "Harness Studio from the command line.\n\n\
When a Harness instance runs (the GUI with the Control API on, or `harness serve`), commands go \
through its Control API. Otherwise library commands run a temporary in-process instance that \
owns the data directory for the duration of the command; commands that start llama-server need \
a running instance or --headless.\n\n\
Exit codes: 0 ok, 1 error, 2 usage, 3 not found, 4 conflict, 5 out of memory, 6 connection."
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

/// `--data-dir` help with this OS's default (the GUI's app data dir).
#[cfg(windows)]
const DATA_DIR_HELP: &str =
    "Data directory (default: the GUI's, e.g. %APPDATA%\\com.harnessstudio.app)";
#[cfg(target_os = "macos")]
const DATA_DIR_HELP: &str =
    "Data directory (default: the GUI's, e.g. ~/Library/Application Support/com.harnessstudio.app)";
#[cfg(not(any(windows, target_os = "macos")))]
const DATA_DIR_HELP: &str =
    "Data directory (default: the GUI's, e.g. ~/.local/share/com.harnessstudio.app)";

#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// Machine-readable output: data as JSON on stdout, errors as JSON on stderr.
    #[arg(long, global = true)]
    pub json: bool,
    /// Answer yes to confirmations (required for destructive commands without a terminal).
    #[arg(long, short = 'y', global = true)]
    pub yes: bool,
    /// No progress output on stderr.
    #[arg(long, short = 'q', global = true)]
    pub quiet: bool,
    #[arg(
        long,
        global = true,
        env = "HARNESS_DATA_DIR",
        value_name = "DIR",
        help = DATA_DIR_HELP
    )]
    pub data_dir: Option<PathBuf>,
    /// Control API of a remote instance (`host:port` or a URL); needs a token.
    #[arg(long, global = true, env = "HARNESS_HOST", value_name = "HOST")]
    pub host: Option<String>,
    /// Token for --host (a named token; the local session token is found automatically).
    #[arg(
        long,
        global = true,
        env = "HARNESS_TOKEN",
        hide_env_values = true,
        value_name = "TOKEN"
    )]
    pub token: Option<String>,
    /// Without a running instance: start a temporary one for this command (servers it starts
    /// stop when the command exits).
    #[arg(long, global = true)]
    pub headless: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Train a local model and export merged weights or LoRA adapters.
    Training {
        #[command(subcommand)]
        command: TrainingCommand,
    },
    /// Instance, runtime, hardware and server summary.
    Status,
    /// Removes this executable's folder from the user PATH (the uninstaller calls it: NSIS
    /// strings stop at 1024 characters, a developer PATH is often longer).
    #[command(hide = true, name = "remove-from-path")]
    RemoveFromPath,
    /// Offline cleanup used by Settings and the Windows uninstaller. Keeps user data by default.
    #[command(hide = true)]
    Cleanup {
        #[arg(long, default_value = "com.harnessstudio.app")]
        identifier: String,
        #[arg(long)]
        delete_data: bool,
        #[arg(long)]
        wait_pid: Option<u32>,
        #[arg(long)]
        external_model: Vec<PathBuf>,
    },
    /// Check the installation: runtime, data directory, ports, GPU devices.
    Doctor,
    /// List local models (and the model definitions built on them).
    #[command(visible_alias = "list")]
    Ls,
    /// Show one model: file, GGUF metadata, definitions.
    Show {
        /// Model name, file name, path or definition alias.
        model: String,
    },
    /// Search Hugging Face for GGUF models.
    Search {
        query: String,
        /// Maximum results.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// downloads | likes | trendingScore | lastModified | createdAt
        #[arg(long)]
        sort: Option<String>,
    },
    /// Download a model from Hugging Face (`owner/repo[:quant]`).
    Pull {
        /// `owner/repo`, `owner/repo:Q4_K_M` or a huggingface.co URL.
        model: String,
        /// A file of the variant (takes precedence over the quantization).
        #[arg(long)]
        file: Option<String>,
        /// Queue it and return at once (needs a running instance).
        #[arg(long)]
        detach: bool,
    },
    /// Delete a local model (all its files).
    Rm {
        model: String,
        /// Also delete the model definitions based on it.
        #[arg(long)]
        definitions: bool,
    },
    /// List running servers.
    Ps {
        /// Also list stopped servers and profiles.
        #[arg(long, short = 'a')]
        all: bool,
    },
    /// Load a model on a server (a new one unless --server).
    Load {
        /// Definition alias, model name or path to a .gguf file.
        model: String,
        /// Server instance or profile to load it on.
        #[arg(long)]
        server: Option<String>,
        /// Return once the start was accepted (do not wait until it is ready).
        #[arg(long)]
        no_wait: bool,
        /// Seconds to wait for the server to become ready (0: until the server's own load timeout
        /// ends the start; Settings > Server defaults or the model's `load_timeout`).
        #[arg(long, default_value_t = 0)]
        timeout: u64,
    },
    /// Start a failed server again. Without --option/--set: show why it failed and the ways
    /// to retry (after an out-of-memory failure: --fit, lower context / GPU layers / -ub, a
    /// quantized KV cache, MoE experts on the CPU, stopping another server).
    Retry {
        /// Server id.
        server: String,
        /// A recovery option id (as listed by `harness retry <server>`).
        #[arg(long)]
        option: Option<String>,
        /// A catalog parameter for this start, `id=value` (repeatable), e.g.
        /// `--set context_size=8192`. Numbers and true/false are typed; other values are text.
        #[arg(long = "set", value_name = "ID=VALUE")]
        set: Vec<String>,
        /// Stop this server first (repeatable).
        #[arg(long)]
        unload: Vec<String>,
        /// Also save the values to the model definition (later starts use them).
        #[arg(long)]
        save: bool,
        /// Return once the start was accepted.
        #[arg(long)]
        no_wait: bool,
        /// Seconds to wait (0: until the server's own load timeout ends the start).
        #[arg(long, default_value_t = 0)]
        timeout: u64,
    },
    /// Stop the servers serving a model (or one server).
    Unload {
        /// Definition alias or id.
        model: Option<String>,
        #[arg(long)]
        server: Option<String>,
    },
    /// Stop servers.
    Stop {
        /// Server ids.
        servers: Vec<String>,
        /// Stop every running server.
        #[arg(long)]
        all: bool,
    },
    /// LoRA adapters of a running server: list them, or change their scales without a restart.
    Lora {
        #[command(subcommand)]
        command: LoraCommand,
    },
    /// Run a prompt once and stream the answer (no prompt: read stdin, or chat in a terminal).
    Run(RunArgs),
    /// Interactive chat with a model (`/bye` or Ctrl+D to leave, `/clear` to reset).
    Chat(ChatArgs),
    /// Generate an image with an image model (stable-diffusion.cpp) through the gateway.
    Image(ImageArgs),
    /// Run headless: own the data directory, serve the Control API, restore servers, run until
    /// Ctrl+C.
    Serve(ServeArgs),
    /// Ask a headless instance (`harness serve`) to shut down cleanly.
    Shutdown,
    /// Server profiles.
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Models of a router server (one llama-server process, many models, loaded on demand).
    Router {
        #[command(subcommand)]
        command: RouterCommand,
    },
    /// llama-bench runs.
    Benchmark {
        #[command(subcommand)]
        command: BenchmarkCommand,
    },
    /// Server log.
    Logs {
        /// Server id.
        server: String,
        /// Follow new lines until Ctrl+C.
        #[arg(long, short = 'f')]
        follow: bool,
        /// Show the last N lines.
        #[arg(long, short = 'n', default_value_t = 100)]
        lines: usize,
    },
    /// llama-server runtimes: the official one plus forks / local builds chosen per server.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    /// Optional modules: list them, pack an installed one into an offline `.hsmod` bundle,
    /// install an official bundle with no network, remove a module.
    Modules {
        #[command(subcommand)]
        command: ModulesCommand,
    },
    /// Named Control API tokens: scopes, allowed models/endpoints, rate limits.
    #[command(name = "api-token", visible_alias = "token")]
    ApiToken {
        #[command(subcommand)]
        command: TokenCommand,
    },
    /// OpenTelemetry (OTLP) export of gateway traces and metrics.
    Otel {
        #[command(subcommand)]
        command: OtelCommand,
    },
    /// Tools Harness runs itself (read/search/write files, edit, shell), under the tool
    /// policy: list them, or run one.
    Tools {
        #[command(subcommand)]
        command: ToolsCommand,
    },
    /// Trust levels and grants of MCP servers, agent runtimes and the training backend
    /// (nothing is granted by default; every change is written to the tool log).
    Permissions {
        #[command(subcommand)]
        command: PermissionsCommand,
    },
    /// Agent Skills: list, switch on or off, remove, or install an official one.
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// Run `harness serve` at login as an OS service (systemd --user, launchd, Task Scheduler).
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Print a shell completion script.
    Completion { shell: clap_complete::Shell },
    /// Harness MCP server on stdin/stdout (for Claude Code, Codex, OpenCode, Cursor...).
    Mcp(McpArgs),
    /// Gateway request statistics per model: runs, tokens, t/s, P50/P95 TTFT and latency,
    /// VRAM, error/OOM rate, clients, benchmark vs real use (managed-endpoint traffic only).
    Analytics {
        /// 24h, 7d, 30d, 90d or all.
        #[arg(long, default_value = "7d")]
        range: String,
    },
    /// Recent gateway requests (request log), or one request with its timeline.
    Requests {
        /// A request id (`X-Harness-Request-Id`): show that request in detail.
        id: Option<String>,
        /// Only this model alias.
        #[arg(long)]
        model: Option<String>,
        /// Only this client label.
        #[arg(long)]
        client: Option<String>,
        /// ok, error, oom or cancelled.
        #[arg(long)]
        status: Option<String>,
        /// Only tool rounds with a tool-call finding (who is at fault: runtime, context, model,
        /// client).
        #[arg(long)]
        tool_issue: bool,
        /// Only findings of this category: runtime, context, model or client.
        #[arg(long)]
        tool_category: Option<String>,
        /// Show the last N requests.
        #[arg(long, short = 'n', default_value_t = 20)]
        limit: u32,
    },
    /// Stored chats of the Playground: list, show, export, documents, context compaction
    /// (needs the `conversations` scope).
    #[command(visible_alias = "chats")]
    Conversations {
        #[command(subcommand)]
        command: ConversationCommand,
    },
    /// Start a coding agent (Claude Code, Codex, OpenCode, Aider, Goose) on a Harness model.
    Launch(LaunchArgs),
    /// Import from other tools.
    Import {
        #[command(subcommand)]
        command: ImportCommand,
    },
    /// Hugging Face account (search, gated / private repos and downloads use it).
    Hf {
        #[command(subcommand)]
        command: HfCommand,
    },
    /// Read text aloud with a local text-to-speech model (sherpa-onnx; needs no running instance).
    #[command(
        long_about = "Turns text into speech with a local model and the sherpa-onnx runtime, in this process (no server needs to be running). The text is the argument, or stdin when the argument is - or missing. The audio is written to -o (default: speech.<format> in the current folder; - writes it to stdout). Install a voice with `harness tts install <id>` and the runtime with `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`."
    )]
    Speak(SpeakArgs),
    /// Text-to-speech models: list the installed ones and the recommended downloads, install, remove.
    Tts {
        #[command(subcommand)]
        command: TtsCommand,
    },
    /// Transcribe an audio file with a local speech-recognition model (sherpa-onnx or ONNX CTC; needs no running instance).
    #[command(
        long_about = "Turns speech into text with a local ONNX model (NVIDIA Parakeet, SenseVoice, Omnilingual...) and the sherpa-onnx runtime, in this process (no server needs to be running). For sherpa-onnx models, long recordings are cut at pauses, so every segment has a time. ONNX CTC models from the library (wav2vec2, HuBERT, WavLM) are also accepted via -m MODEL and run with the managed ONNX Runtime engine. CTC models only transcribe, ignore prompts and language hints, and do not provide timed segments. The text goes to stdout (or -o FILE) as plain text, json, verbose_json (with the timed segments), srt or vtt. Install a model with `harness asr install <id>` and the runtime with `harness runtime add --github k2-fsa/sherpa-onnx --family sherpa-onnx`. Whisper models (ggml .bin) are served by the gateway, not by this command."
    )]
    Transcribe(TranscribeArgs),
    /// Speech-recognition models of sherpa-onnx: list the installed ones and the recommended downloads, install, remove.
    Asr {
        #[command(subcommand)]
        command: AsrCommand,
    },
    /// Lemonade, the optional engine for Ryzen AI NPUs and its own llama.cpp backends (needs no running instance).
    #[command(
        long_about = "Lemonade (lemonade-sdk/lemonade, Apache-2.0) runs models on AMD Ryzen AI NPUs through FastFlowLM and offers its own ROCm / Vulkan llama.cpp builds as an alternative backend. It does not replace Harness's runtimes: install it with `harness lemonade install`, then its downloaded models are served by the gateway under their Lemonade names (the gateway starts `lemond` on demand). `pull`, `rm`, `backend` and `models --refresh` start a `lemond` of their own for the time of the command."
    )]
    Lemonade {
        #[command(subcommand)]
        command: LemonadeCommand,
    },
    /// Convert a Hugging Face safetensors model to GGUF and add it to the library (needs no running instance).
    #[command(
        args_conflicts_with_subcommands = true,
        long_about = "Downloads a safetensors LLM repo (text-generation or image-text-to-text) from Hugging Face, converts it with llama.cpp's official converter, quantizes it with the managed runtime's llama-quantize and places the GGUF in <models>/<owner>/<repo>-GGUF/, where the library finds it. Python is used only for the conversion, from a throwaway environment Harness builds itself the first time (about 700 MB: uv, Python, torch CPU, transformers; see `harness convert env`). Needs the managed llama.cpp runtime. Gated repos need `harness hf login`. Disk need: about twice the weights plus the result (shown before it starts; --plan only shows it)."
    )]
    Convert(ConvertArgs),
}

#[derive(Subcommand, Debug)]
pub enum TrainingCommand {
    /// List persisted training jobs.
    List,
    /// Create and start a job from a JSON CreateRequest spec. Needs a running instance.
    Start { spec: PathBuf },
    /// Show a job, including export progress and output paths.
    Status { id: u64 },
    /// Cancel a running job, saving a checkpoint first.
    Cancel { id: u64 },
    /// Resume from the last complete checkpoint. Needs a running instance.
    Resume { id: u64 },
    /// Export weights to the Library. Needs a running instance.
    Export {
        id: u64,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        gguf: bool,
        #[arg(long, default_value = "Q4_K_M", value_parser = ["F16", "Q8_0", "Q4_K_M", "Q5_K_M"])]
        quant: String,
        #[arg(long)]
        adapter_only: bool,
    },
}

#[derive(clap::Args, Debug)]
pub struct ConvertArgs {
    /// Hugging Face repo with safetensors weights (`owner/repo` or a huggingface.co URL).
    pub repo: Option<String>,
    /// Target type: Q4_K_M (default), Q5_K_M, Q6_K, Q8_0, F16, BF16, or any type llama-quantize lists.
    #[arg(long, default_value = "Q4_K_M")]
    pub quant: String,
    /// Keep the downloaded safetensors in the library after the conversion.
    #[arg(long)]
    pub keep_original: bool,
    /// Only show what the conversion would take (architecture, sizes, disk), then stop.
    #[arg(long)]
    pub plan: bool,
    #[command(subcommand)]
    pub command: Option<ConvertCommand>,
}

#[derive(Subcommand, Debug)]
pub enum ConvertCommand {
    /// The conversion environment (uv + Python + converter packages): state, build, removal.
    Env {
        #[command(subcommand)]
        command: ConvertEnvCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum LemonadeCommand {
    /// Installed builds, the NPU, the llama.cpp backend setting and the backends of the last run.
    Status,
    /// Download a Lemonade release (checked against GitHub's SHA-256) and register it.
    Install {
        /// Release tag (default: the newest stable release).
        #[arg(long)]
        tag: Option<String>,
    },
    /// Remove an installed build (its id is in `harness lemonade status --json`).
    Uninstall {
        /// Runtime id.
        id: String,
    },
    /// Lemonade's model registry (the last snapshot, or the current one with --refresh).
    Models {
        /// Start lemond for a moment and read the current registry.
        #[arg(long)]
        refresh: bool,
        /// Only the downloaded models.
        #[arg(long)]
        downloaded: bool,
    },
    /// Download a model into Lemonade (a registry name such as Qwen3-0.6B-GGUF, or owner/repo[:variant]).
    Pull {
        /// Model name or checkpoint.
        model: String,
    },
    /// Delete a downloaded model from Lemonade.
    Rm {
        /// Model name.
        model: String,
    },
    /// Install a backend Lemonade runs models on (llamacpp rocm, llamacpp vulkan, flm npu...).
    Backend {
        /// Recipe: llamacpp, flm, whispercpp.
        recipe: String,
        /// Backend: vulkan, rocm, cpu, npu.
        backend: String,
    },
    /// Set the llama.cpp backend (auto | vulkan | rocm | cpu) and/or pin the build to run (an id, or `default`).
    Config {
        #[arg(long)]
        backend: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConvertEnvCommand {
    /// Whether the environment exists, for which llama.cpp tag, and its size.
    Status,
    /// Build it now (otherwise the first conversion does).
    Install,
    /// Delete it (about 700 MB come back; the next conversion builds it again).
    Rm,
}

#[derive(clap::Args, Debug)]
pub struct TranscribeArgs {
    /// The audio file (WAV; other formats need ffmpeg).
    pub file: PathBuf,
    /// Installed sherpa-onnx model id/name or unique name part, or an ONNX CTC library model
    /// (wav2vec2, HuBERT, WavLM) by definition id/alias, library id/name or exact file path
    /// (default: the first sherpa-onnx model, or the first library CTC model if none is installed).
    #[arg(long, short = 'm')]
    pub model: Option<String>,
    /// Spoken language (SenseVoice takes zh, en, ja, ko, yue; Cohere Transcribe needs one of its 14;
    /// the other models find it themselves).
    #[arg(long, short = 'l')]
    pub language: Option<String>,
    /// Tell the speakers apart (each segment says who spoke: A, B...).
    #[arg(long)]
    pub diarize: bool,
    /// How many speakers there are, when you know (implies --diarize).
    #[arg(long)]
    pub speakers: Option<u32>,
    /// text | json | verbose_json | diarized_json | srt | vtt.
    #[arg(long, short = 'f', default_value = "text")]
    pub format: String,
    /// Output file (default: the text goes to stdout).
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
    /// Runtime id of the sherpa-onnx build (default: the newest installed).
    #[arg(long)]
    pub runtime: Option<String>,
    /// CPU threads of the recognition (default: half of the cores).
    #[arg(long)]
    pub threads: Option<u32>,
}

#[derive(Subcommand, Debug)]
pub enum AsrCommand {
    /// Installed speech-recognition models (sherpa-onnx).
    Ls,
    /// Models on offer (Parakeet, SenseVoice, Omnilingual with Turkish) with size and licence.
    Catalog,
    /// Download a catalog model into the models folder (checked against its SHA-256).
    Install {
        /// Catalog id (see `harness asr catalog`).
        id: String,
    },
    /// Remove an installed speech-recognition model (its whole folder).
    Rm {
        /// Model id or name.
        model: String,
    },
}

#[derive(clap::Args, Debug)]
pub struct SpeakArgs {
    /// The text to read (- or nothing: read stdin).
    pub text: Option<String>,
    /// Model id, name or a voice name of the installed models (default: the first installed one;
    /// OpenAI names such as tts-1 also pick the default).
    #[arg(long, short = 'm')]
    pub model: Option<String>,
    /// Voice name or speaker number (default: the model's own default; OpenAI names such as
    /// alloy map to Kokoro voices).
    #[arg(long)]
    pub voice: Option<String>,
    /// Speaking speed, 0.25 to 4.0.
    #[arg(long, short = 's')]
    pub speed: Option<f32>,
    /// wav | pcm | mp3 | opus | aac | flac (everything but wav and pcm needs ffmpeg).
    #[arg(long, short = 'f', default_value = "wav")]
    pub format: String,
    /// Output file (default: speech.<format>; - writes the audio to stdout).
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
    /// Runtime id of the sherpa-onnx build (default: the newest installed).
    #[arg(long)]
    pub runtime: Option<String>,
    /// CPU threads of the synthesis (default: half of the cores).
    #[arg(long)]
    pub threads: Option<u32>,
}

#[derive(Subcommand, Debug)]
pub enum TtsCommand {
    /// Installed text-to-speech models with their voices.
    Ls,
    /// Models on offer (Kokoro multi-lingual, Turkish Piper voices) with size and licence.
    Catalog,
    /// Download a catalog model into the models folder (checked against its SHA-256).
    Install {
        /// Catalog id (see `harness tts catalog`).
        id: String,
    },
    /// Remove an installed text-to-speech model (its whole folder).
    Rm {
        /// Model id or name.
        model: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum LoraCommand {
    /// List the adapters the server loaded with their ids and current scales.
    #[command(visible_alias = "ls")]
    List {
        /// Server id.
        server: String,
    },
    /// Set the scale of one adapter (0 turns it off); the other adapters keep their scales.
    Set {
        /// Server id.
        server: String,
        /// Adapter id as listed by `harness lora list`.
        id: u32,
        /// New scale (the GUI offers 0 to 2).
        scale: f64,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConversationCommand {
    /// List conversations, newest first.
    Ls {
        /// Search titles and message contents.
        query: Option<String>,
        /// Only this folder id.
        #[arg(long)]
        folder: Option<String>,
        #[arg(long)]
        pinned: bool,
        #[arg(long)]
        favorites: bool,
        /// Show at most N conversations.
        #[arg(long, short = 'n', default_value_t = 50)]
        limit: u32,
    },
    /// Show a conversation: its current path of messages.
    Show { id: String },
    /// Export a conversation: JSON (`harness.conversation.v1`, the file the app imports) or
    /// Markdown.
    Export {
        id: String,
        /// json | markdown
        #[arg(long, short = 'f', default_value = "json")]
        format: String,
        /// Output file (default: stdout).
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
    /// Documents attached to a conversation (metadata only).
    Docs { id: String },
    /// Replace the older turns of a conversation with a model-written summary (needs a
    /// running instance with the gateway on: the summary is requested through it).
    Compact {
        id: String,
        /// Gateway alias that writes the summary (default: the conversation's model).
        #[arg(long, short = 'm')]
        model: Option<String>,
        /// Context window of the chat model in tokens (default: the loaded server's).
        #[arg(long)]
        n_ctx: Option<u64>,
        /// Newest turns kept as they are.
        #[arg(long)]
        keep_turns: Option<usize>,
        /// Do not shorten old tool results before summarizing.
        #[arg(long)]
        keep_tool_results: bool,
        /// Summary budget in tokens.
        #[arg(long)]
        summary_tokens: Option<u64>,
        /// Extra instruction for the summary ("keep the numbers").
        #[arg(long, short = 'i')]
        instruction: Option<String>,
    },
    /// Undo a compaction (requests send the full history again); --redo applies it again.
    Undo {
        id: String,
        /// The marker message id `compact` printed.
        marker: String,
        #[arg(long)]
        redo: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum HfCommand {
    /// Sign in with Hugging Face (browser, OAuth + PKCE) or save an access token (--with-token).
    #[command(
        long_about = "Signs in with Hugging Face. By default the browser opens huggingface.co and the authorization comes back to a one-time listener on 127.0.0.1 (OAuth authorization code + PKCE); --device prints a code to enter on huggingface.co/oauth/device instead (for machines without a browser). Both need the client id of a public OAuth app registered on huggingface.co (--client-id, Settings > Storage > Hugging Face account, or HARNESS_HF_OAUTH_CLIENT_ID). --with-token saves an access token read from stdin instead (checked against huggingface.co first). The token is stored in the OS keychain, never in a file; the GUI and `harness serve` use the same one."
    )]
    Login {
        /// Read an access token from stdin instead of the browser sign-in.
        #[arg(long, conflicts_with_all = ["device", "client_id"])]
        with_token: bool,
        /// Device code flow: enter a short code on huggingface.co (no local browser needed).
        #[arg(long)]
        device: bool,
        /// OAuth app client id (default: Settings, then HARNESS_HF_OAUTH_CLIENT_ID).
        #[arg(long, value_name = "ID")]
        client_id: Option<String>,
        /// Print the address instead of opening a browser.
        #[arg(long)]
        no_browser: bool,
    },
    /// Remove the saved token / sign-in from the OS keychain.
    Logout,
    /// Show the signed-in account (checked online).
    Whoami,
}

#[derive(Subcommand, Debug)]
pub enum ImportCommand {
    /// Import an Ollama Modelfile as a model definition plus an inference preset.
    #[command(
        long_about = "Reads an Ollama Modelfile (FROM, PARAMETER, SYSTEM, ADAPTER; TEMPLATE and MESSAGE are reported and skipped) and creates a model definition and an inference preset. FROM may be a .gguf path (relative paths start at the Modelfile's directory) or the name of a model installed in Ollama (its GGUF blob is used in place, read-only, and its stored parameters and system prompt are inherited). Lists what was mapped and what was not."
    )]
    Modelfile {
        /// Modelfile (`-` = stdin). Omit with --ollama.
        #[arg(required_unless_present = "ollama", conflicts_with = "ollama")]
        file: Option<PathBuf>,
        /// Import an installed Ollama model by name (`qwen3:30b-a3b`) instead of a file.
        #[arg(long, value_name = "NAME")]
        ollama: Option<String>,
        /// Model definition alias (default: derived from FROM; a free name when taken).
        #[arg(long)]
        alias: Option<String>,
        /// Only show what would be created.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Args, Debug)]
#[command(
    long_about = "Starts a coding agent on a Harness model with process-only settings: nothing in the tool's global configuration is written.

The tool talks to the managed endpoint (gateway) while it runs, else to the model's llama-server. When the model's context is below what the tool needs, `harness launch` offers to prepare the server (the definition gets the largest context the model supports and the GPU's free memory holds now (idle models the gateway can evict count as free; its headroom stays free) within the tool's limits, flash attention, an f16 KV cache (8-bit q8_0, then K q8_0 + V q4_0, only when f16 does not fit) and one slot, then reloads; the choice, its reason and alternatives are printed first). Arguments after `--` go to the tool, e.g. `harness launch codex -m qwen3 -- exec \"fix the tests\"`. `harness` exits with the tool's exit code."
)]
pub struct LaunchArgs {
    /// The tool to start.
    #[arg(value_enum)]
    pub tool: LaunchToolArg,
    /// Definition alias, model name or .gguf file (default: pick one; without a terminal the
    /// only running model).
    #[arg(long, short = 'm')]
    pub model: Option<String>,
    /// Print what would run (env, args, temp files; secrets redacted) and start nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Keep OpenCode external Claude/agents skills (larger prompts, slower turns). Hidden by default.
    #[arg(long)]
    pub keep_skills: bool,
    /// Prepare the server without asking when the context is too small.
    #[arg(long, conflicts_with = "no_prepare")]
    pub prepare: bool,
    /// Start even when the context is below the tool's minimum.
    #[arg(long)]
    pub no_prepare: bool,
    /// Use the llama-server port even while the gateway runs.
    #[arg(long)]
    pub direct: bool,
    /// API key of the llama-server (direct endpoint of a server bound with a key).
    #[arg(long, env = "HARNESS_SERVER_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,
    /// Seconds to wait for the model to load (0: the server's own load timeout).
    #[arg(long, default_value_t = 0)]
    pub timeout: u64,
    /// Arguments for the tool (after `--`).
    #[arg(last = true)]
    pub args: Vec<String>,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchToolArg {
    #[value(alias = "claude-code")]
    Claude,
    Codex,
    Opencode,
    Aider,
    Goose,
}

impl From<LaunchToolArg> for harness_core::integrations::launch::LaunchTool {
    fn from(t: LaunchToolArg) -> Self {
        use harness_core::integrations::launch::LaunchTool as T;
        match t {
            LaunchToolArg::Claude => T::Claude,
            LaunchToolArg::Codex => T::Codex,
            LaunchToolArg::Opencode => T::OpenCode,
            LaunchToolArg::Aider => T::Aider,
            LaunchToolArg::Goose => T::Goose,
        }
    }
}

#[derive(Args, Debug)]
#[command(
    long_about = "Runs the Harness MCP server over stdio: MCP clients (Claude Code, Codex, OpenCode, Cursor...) start `harness mcp` and call its tools (list_models, load_model, get_endpoint...).

Tools go through the running instance's Control API (the GUI with Settings > Developer > Control API on, or `harness serve`); with --headless and no running instance a temporary instance lives as long as this process. stdout carries only MCP messages."
)]
pub struct McpArgs {
    /// Scopes the tools may use: read, servers:write (load/unload/start/stop/benchmark),
    /// models:write (pull/delete models), conversations (read and compact stored chats),
    /// tools (Harness's own file/shell/network tools, offered only when Settings > Tools >
    /// "Offer tools to MCP clients" is on), admin. Comma-separated.
    #[arg(
        long,
        value_name = "SCOPES",
        default_value = "read,servers:write,tools"
    )]
    pub scope: String,
}

#[derive(Args, Debug, Clone)]
pub struct GenerationArgs {
    /// System prompt.
    #[arg(long)]
    pub system: Option<String>,
    /// Maximum tokens to generate.
    #[arg(long)]
    pub max_tokens: Option<u32>,
    #[arg(long)]
    pub temperature: Option<f32>,
    /// API key of the llama-server (servers bound with a key).
    #[arg(long, env = "HARNESS_SERVER_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,
    /// Seconds to wait for the model to load (0: until the server's own load timeout ends the
    /// start).
    #[arg(long, default_value_t = 0)]
    pub timeout: u64,
}

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Definition alias, model name or path to a .gguf file.
    pub model: String,
    /// The prompt (words are joined). Omitted: stdin, or an interactive chat in a terminal.
    pub prompt: Vec<String>,
    /// Stop the server afterwards if this command loaded it.
    #[arg(long)]
    pub unload: bool,
    #[command(flatten)]
    pub gen: GenerationArgs,
}

#[derive(Args, Debug)]
pub struct ImageArgs {
    /// What to draw (words are joined). Omitted: read from stdin.
    pub prompt: Vec<String>,
    /// Image model: definition alias (a stable-diffusion.cpp model).
    #[arg(long, short = 'm')]
    pub model: String,
    /// File to write (default `image.png`); several images are numbered `name-1.png`...
    #[arg(long, short = 'o')]
    pub output: Option<std::path::PathBuf>,
    /// Size `WIDTHxHEIGHT` (a multiple of 8); default: the model's own.
    #[arg(long)]
    pub size: Option<String>,
    /// What the picture must not show.
    #[arg(long)]
    pub negative: Option<String>,
    /// Sampling steps.
    #[arg(long)]
    pub steps: Option<u32>,
    /// Classifier-free guidance scale.
    #[arg(long = "cfg-scale")]
    pub cfg_scale: Option<f64>,
    /// Seed (-1: random).
    #[arg(long, allow_hyphen_values = true)]
    pub seed: Option<i64>,
    /// Sampler (`euler`, `euler_a`, `dpm++2m`...).
    #[arg(long)]
    pub sampler: Option<String>,
    /// How many images.
    #[arg(long, short = 'n', default_value_t = 1)]
    pub count: u32,
    /// png, jpeg or webp.
    #[arg(long)]
    pub format: Option<String>,
    /// Seconds to wait for the answer (model load included).
    #[arg(long, default_value_t = 1800)]
    pub timeout: u64,
}

#[derive(Args, Debug)]
pub struct ChatArgs {
    pub model: String,
    #[arg(long)]
    pub unload: bool,
    #[command(flatten)]
    pub gen: GenerationArgs,
}

#[derive(Args, Debug)]
pub struct ServeArgs {
    /// JSON serve configuration (ui_dir, public_origin, tls_cert, tls_key).
    #[arg(long, value_name = "FILE")]
    pub serve_config: Option<PathBuf>,
    /// Static Svelte build directory (default: web beside the executable, then ./build).
    #[arg(long, value_name = "DIR")]
    pub ui_dir: Option<PathBuf>,
    /// Address the Control API binds (default: settings, 127.0.0.1).
    #[arg(long, value_name = "HOST")]
    pub listen: Option<String>,
    /// Control API port (default: settings, 11500). A port from the settings that another
    /// program holds is replaced by a fallback port; one given here fails instead.
    #[arg(long)]
    pub port: Option<u16>,
    /// Do not start auto-start profiles / the last session.
    #[arg(long)]
    pub no_restore: bool,
    /// Also run the gateway (managed endpoint, default 127.0.0.1:11435) even when it is off in
    /// the settings.
    #[arg(long, conflicts_with = "no_gateway")]
    pub gateway: bool,
    /// Do not run the gateway even when it is on in the settings.
    #[arg(long)]
    pub no_gateway: bool,
    /// Gateway port (default: settings, 11435; a fallback port when another program holds it).
    /// A port given here fails instead when taken.
    #[arg(long, value_name = "PORT")]
    pub gateway_port: Option<u16>,
}

#[derive(Subcommand, Debug)]
pub enum RouterCommand {
    /// List the models of a router server and whether each is loaded.
    Models {
        /// Router server id.
        server: String,
    },
    /// Load a model of a router server.
    Load {
        /// Router server id.
        server: String,
        /// Model name as listed by `router models`.
        model: String,
        /// Return at once (do not wait until the model is loaded).
        #[arg(long)]
        no_wait: bool,
        /// Seconds to wait for the model.
        #[arg(long, default_value_t = 300)]
        timeout: u64,
    },
    /// Unload a model of a router server.
    Unload {
        /// Router server id.
        server: String,
        /// Model name as listed by `router models`.
        model: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProfileCommand {
    /// List server profiles.
    Ls,
    /// Show a profile.
    Show { id: String },
    /// Start a profile.
    Run {
        id: String,
        #[arg(long)]
        no_wait: bool,
        /// Seconds to wait (0: until the server's own load timeout ends the start).
        #[arg(long, default_value_t = 0)]
        timeout: u64,
    },
    /// Import a profile exported with `profile export`.
    Import {
        /// File (`-` = stdin).
        file: PathBuf,
    },
    /// Export a profile (and its model definition) as JSON.
    Export {
        id: String,
        /// Output file (default: stdout).
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BenchmarkCommand {
    /// Run llama-bench on a model and wait for the result.
    Run {
        /// Model name, path or definition alias.
        model: String,
        /// Prompt sizes (comma-separated sweep).
        #[arg(long, value_delimiter = ',', default_value = "512")]
        n_prompt: Vec<u32>,
        /// Generation sizes.
        #[arg(long, value_delimiter = ',', default_value = "128")]
        n_gen: Vec<u32>,
        /// GPU layers (sweep).
        #[arg(long, value_delimiter = ',')]
        ngl: Vec<i32>,
        #[arg(long)]
        repetitions: Option<u32>,
        #[arg(long)]
        threads: Option<u32>,
    },
    /// Saved runs.
    Ls,
    /// One run.
    Show { id: String },
}

#[derive(Subcommand, Debug)]
pub enum ModulesCommand {
    /// List the modules with installed and bundled version and status.
    Ls,
    /// Pack an installed module (components, manifest, SHA-256 per file) into a `.hsmod` bundle.
    /// Prints the bundle's SHA-256, which a maintainer records in the official index.
    Export {
        /// Module id (see `harness modules ls`).
        id: String,
        /// Bundle file to write (use the .hsmod extension).
        #[arg(long, short = 'o', value_name = "FILE")]
        out: PathBuf,
    },
    /// Install a module from an official `.hsmod` bundle. Uses no network. Bundles that are not
    /// in the official index shipped with this build are refused.
    Import {
        /// Bundle file.
        file: PathBuf,
    },
    /// Remove an installed module (models and your data stay). Needs no network.
    Rm { id: String },
}

#[derive(Subcommand, Debug)]
pub enum RuntimeCommand {
    /// List runtimes (official first) with version, backend and capability summary.
    Ls,
    /// Add a runtime from a GitHub release (--github owner/repo) or a local folder (--local DIR).
    #[command(group(clap::ArgGroup::new("source").required(true).args(["github", "local"])))]
    Add {
        /// GitHub repository whose release archives to install (e.g. PrismML-Eng/llama.cpp).
        #[arg(long, value_name = "OWNER/REPO")]
        github: Option<String>,
        /// Folder with llama-server, or the family's binary (used in place unless --copy).
        #[arg(long, value_name = "DIR")]
        local: Option<PathBuf>,
        /// Runtime family: llama.cpp, whisper.cpp, stable-diffusion.cpp, sherpa-onnx or
        /// mistral.rs (local folders only: the folder of a self-built `mistralrs` binary)
        /// (GitHub: taken from the repository name when it is one of those; default llama.cpp).
        #[arg(long)]
        family: Option<String>,
        /// Display name.
        #[arg(long)]
        name: Option<String>,
        /// Runtime id (default: from the name).
        #[arg(long)]
        id: Option<String>,
        /// GitHub: release tag (default: the newest with a matching archive).
        #[arg(long, requires = "github")]
        tag: Option<String>,
        /// GitHub: exact archive name.
        #[arg(long, requires = "github")]
        asset: Option<String>,
        /// GitHub: vulkan, cuda, rocm, sycl, cpu, metal (default: hardware recommendation).
        #[arg(long, requires = "github")]
        backend: Option<String>,
        /// GitHub: install archives that have no SHA-256 digest (after the confirmation).
        #[arg(long, requires = "github")]
        allow_unverified: bool,
        /// Local: copy the folder under the data directory instead of using it in place.
        #[arg(long, requires = "local")]
        copy: bool,
    },
    /// Remove a runtime (downloaded/copied files are deleted; a local folder is kept).
    Rm {
        id: String,
        /// Remove even when definitions/profiles still choose it.
        #[arg(long)]
        force: bool,
    },
    /// Re-hash a runtime's files and run --version.
    Verify {
        id: String,
        /// Local runtime: accept the current files as the new baseline.
        #[arg(long)]
        accept_changes: bool,
    },
    /// Which runtime can load a model (GGUF tensor types), e.g. Bonsai PQ2_0 -> PrismML fork.
    Suggest {
        /// Model name, file, path or definition alias.
        model: String,
    },
    /// The official managed llama.cpp build (Settings > Runtime): status, install/update,
    /// roll back.
    Managed {
        #[command(subcommand)]
        command: ManagedCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ManagedCommand {
    /// Installed and previous version, the backend updates keep, what a rollback restores.
    Status,
    /// Download, verify and activate the newest official build (servers must be stopped).
    Install {
        /// vulkan, rocm, cuda, sycl, cpu, metal: exactly this backend (default: the installed
        /// build's backend, else the hardware recommendation).
        #[arg(long)]
        backend: Option<String>,
        /// Install archives that have no SHA-256 digest.
        #[arg(long)]
        allow_unverified: bool,
    },
    /// Restore the build in the backup slot (the current one takes its place).
    Rollback,
}

/// A token's gateway policy (what it may do on the gateway, `/v1/*` and `/api/*`).
#[derive(Args, Debug, Clone, Default)]
pub struct TokenPolicyArgs {
    /// Allowed model aliases, glob patterns such as `qwen3-*` (repeat or comma-separate;
    /// `*` = every model).
    #[arg(long = "model", value_delimiter = ',', value_name = "PATTERN")]
    pub models: Vec<String>,
    /// Allowed gateway endpoints: chat, embeddings, rerank, models, messages, ollama,
    /// metrics (repeat or comma-separate; `all` = every endpoint).
    #[arg(long = "endpoint", value_delimiter = ',', value_name = "ENDPOINT")]
    pub endpoints: Vec<String>,
    /// Requests per minute (0 = no limit).
    #[arg(long, value_name = "N")]
    pub rpm: Option<u32>,
    /// Prompt + completion tokens per minute (0 = no limit).
    #[arg(long, value_name = "N")]
    pub tpm: Option<u32>,
    /// Requests in flight at once (0 = no limit).
    #[arg(long, value_name = "N")]
    pub concurrency: Option<u32>,
}

#[derive(Subcommand, Debug)]
pub enum TokenCommand {
    /// Create a token (the value is shown once).
    Create {
        name: String,
        /// read, inference, models:write, downloads:write, servers:write, servers:control,
        /// conversations, tools, admin (repeat or comma-separate).
        #[arg(long = "scope", value_delimiter = ',', required = true)]
        scopes: Vec<String>,
        #[command(flatten)]
        policy: TokenPolicyArgs,
    },
    /// List tokens with their scopes and policy (never their values).
    Ls,
    /// One token's scopes, allowed models/endpoints and limits.
    Show { id: String },
    /// Change a token's name, scopes, allowed models/endpoints or limits (only the flags
    /// given change; the token value stays the same).
    Set {
        id: String,
        #[arg(long)]
        name: Option<String>,
        /// Replaces the scopes (repeat or comma-separate).
        #[arg(long = "scope", value_delimiter = ',')]
        scopes: Vec<String>,
        #[command(flatten)]
        policy: TokenPolicyArgs,
    },
    /// Revoke a token.
    Rm { id: String },
}

#[derive(Subcommand, Debug)]
pub enum OtelCommand {
    /// Export settings (header names only) and exporter counters.
    Status,
    /// Send one real test span to the saved endpoint (or the one given) and report the
    /// collector's answer.
    Test {
        /// Collector URL to test instead of the saved one.
        #[arg(long)]
        endpoint: Option<String>,
        /// `http` (OTLP/HTTP protobuf) or `grpc`.
        #[arg(long)]
        protocol: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ServiceCommand {
    /// Install and enable the service for the current user.
    Install(ServiceArgs),
    /// Disable and remove it.
    Uninstall(ServiceArgs),
    /// Print the unit / plist / task definition without installing.
    Print(ServiceArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ServiceArgs {
    /// Service name (systemd unit / launchd label / scheduled task name).
    #[arg(long)]
    pub name: Option<String>,
    /// Target platform for `print` (default: this OS).
    #[arg(long, value_enum)]
    pub platform: Option<ServicePlatform>,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServicePlatform {
    Systemd,
    Launchd,
    Windows,
}

#[derive(Subcommand, Debug)]
pub enum ToolsCommand {
    /// Categorised inventory including disabled installed packages.
    Inventory,
    /// Persistent autonomous tool creator (admin).
    Auto {
        #[command(subcommand)]
        command: AutoToolsCommand,
    },
    /// List the tools: permission class, source, and what the tool policy does with each
    /// (run, ask = needs your approval, refuse).
    #[command(visible_alias = "ls")]
    List,
    /// Harness's own MCP client: the servers of `mcp_servers.json` with their state, the MCP
    /// version they negotiated and how many tools each registered. Starts them on first use.
    Mcp {
        /// Try again servers that failed to start.
        #[arg(long)]
        retry: bool,
    },
    /// The resources (and resource templates) an MCP server offers; with `--read <uri>` the
    /// contents of one resource.
    McpResources {
        /// Server name from `mcp_servers.json`.
        server: String,
        /// Read this resource instead of listing them.
        #[arg(long, value_name = "URI")]
        read: Option<String>,
    },
    /// Sign a remote MCP server in (OAuth, MCP authorization spec): opens your browser, waits
    /// for you to approve and stores the token in the OS keychain.
    McpAuthorize {
        /// Server name from `mcp_servers.json`.
        server: String,
    },
    /// Forget the OAuth sign-in of a remote MCP server.
    McpLogout {
        /// Server name from `mcp_servers.json`.
        server: String,
    },
    /// The official registry: the tools, skills and MCP entries this version can install.
    Registry {
        /// Only this kind: tool, skill or mcp.
        #[arg(long, value_parser = ["tool", "skill", "mcp"])]
        kind: Option<String>,
    },
    /// Install an official tool or skill from the registry. Shows the permissions it declares;
    /// they need your yes (`--yes` approves them), and the API refuses any other set.
    Install {
        /// Registry id (`struklabs.tool.json_format`) or its short name (`json_format`).
        id: String,
    },
    /// The installed tools with their permissions and whether they are on.
    Installed,
    /// Switch an installed tool on.
    Enable {
        /// Installed tool name (see `harness tools installed`).
        name: String,
    },
    /// Switch an installed tool off; it stays installed.
    Disable {
        /// Installed tool name (see `harness tools installed`).
        name: String,
    },
    /// Remove an installed tool (asks first; `--yes` skips the question).
    Remove {
        /// Installed tool name (see `harness tools installed`).
        name: String,
    },
    /// Run one tool. A tool the policy asks about (every file and shell tool under the
    /// default "ask every time" policy) needs your approval: a y/N question on a terminal,
    /// `--yes` without one.
    Run {
        /// Tool name (see `harness tools list`).
        name: String,
        /// The tool's arguments as a JSON object (its `input_schema`).
        #[arg(long, default_value = "{}", value_name = "JSON")]
        args: String,
        /// Working directory of file tools and the shell (default: the current directory).
        /// File tools cannot leave it. With --host it names a folder on that machine.
        #[arg(long, value_name = "DIR")]
        workdir: Option<PathBuf>,
        /// Time limit in seconds (default 120).
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<u64>,
    },
    /// Show or configure the provider used by the provider-independent web_search tool.
    SearchProvider {
        /// none, duckduckgo, searxng, brave, or tavily. Omit every option to show status.
        #[arg(long)]
        provider: Option<String>,
        /// SearXNG base URL.
        #[arg(long)]
        url: Option<String>,
        /// Brave/Tavily API key; stored in the OS keychain, never settings.json.
        #[arg(long)]
        api_key: Option<String>,
    },
}

const SUBJECT_KINDS: [&str; 3] = ["mcp_server", "agent", "trainer"];
const CAPABILITIES: [&str; 6] = ["file", "network", "shell", "service", "account", "device"];

#[derive(Subcommand, Debug)]
pub enum PermissionsCommand {
    /// Every subject with its trust level and grants.
    #[command(visible_alias = "ls")]
    List,
    /// Grant one capability. The training backend takes none; agents take `file` only.
    Grant {
        #[arg(value_parser = SUBJECT_KINDS)]
        kind: String,
        /// MCP server name, or the agent (`hermes`, `openclaw`).
        id: String,
        #[arg(value_parser = CAPABILITIES)]
        capability: String,
        /// Existing absolute folder (`file` only; repeat for up to eight).
        #[arg(long = "path")]
        paths: Vec<String>,
    },
    /// Revoke one grant (`--grant <id>`), or every grant and the trust level of the subject.
    Revoke {
        #[arg(value_parser = SUBJECT_KINDS)]
        kind: String,
        id: String,
        /// The grant id shown by `permissions list`.
        #[arg(long)]
        grant: Option<String>,
    },
    /// Set the trust level (safe, restricted, dangerous) or return to the default.
    Trust {
        #[arg(value_parser = SUBJECT_KINDS)]
        kind: String,
        id: String,
        #[arg(value_parser = ["default", "safe", "restricted", "dangerous"])]
        level: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SkillsCommand {
    /// The installed skills and whether they are on.
    #[command(visible_alias = "ls")]
    List,
    /// Switch a skill on.
    Enable {
        /// Skill name (see `harness skills list`).
        name: String,
    },
    /// Switch a skill off; it stays installed.
    Disable {
        /// Skill name (see `harness skills list`).
        name: String,
    },
    /// Remove a skill (asks first; `--yes` skips the question).
    Remove {
        /// Skill name (see `harness skills list`).
        name: String,
    },
    /// Install an official skill from the registry; same approval as `harness tools install`.
    Install {
        /// Registry id (`struklabs.skill.release-notes`) or its short name (`release-notes`).
        id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum AutoToolsCommand {
    /// Read the current run, queue, drafts and logs.
    Status,
    /// Start a run and wait for completion; Ctrl+C stops the worker.
    Start {
        #[arg(long)]
        endpoint: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..=20))]
        count: u32,
        #[arg(long)]
        resume: bool,
    },
    /// Pause/stop/retry/remove/reorder. Approval is intentionally only offered by the UI/API.
    Action {
        #[arg(value_parser = ["pause", "stop", "retry", "remove", "reorder"])]
        action: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long, value_delimiter = ',')]
        order: Vec<String>,
    },
}

#[cfg(test)]
mod launch_skill_tests {
    use super::*;
    #[test]
    fn keep_skills_flag_and_help() {
        for keep in [false, true] {
            let mut argv = vec!["harness", "launch", "opencode", "--dry-run"];
            if keep {
                argv.push("--keep-skills");
            }
            let cli = Cli::try_parse_from(argv).unwrap();
            let Command::Launch(args) = cli.command else {
                panic!("launch expected")
            };
            assert_eq!(args.keep_skills, keep);
        }
        let help = Cli::try_parse_from(["harness", "launch", "--help"])
            .unwrap_err()
            .to_string();
        assert!(help.contains("--keep-skills"));
        assert!(help.contains("Hidden by default"));
    }
}
