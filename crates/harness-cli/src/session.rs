//! Where commands run: a running instance's Control API, a remote one, or a temporary
//! in-process instance.
//!
//! Discovery (private hs-internal architecture notes): the owner of the data directory holds `instance.lock` and
//! describes itself in `instance.json` (API address, session token file).
//!
//! * `--host` / `HARNESS_HOST`: that API, with `--token` / `HARNESS_TOKEN`.
//! * A live owner with its API on: its API, with the session token from its token file.
//! * A live owner without an API: exit 4 (`cli.owner_without_api`); the CLI never writes the
//!   data directory of another process.
//! * Nobody (or a stale `instance.json`): a temporary instance in this process takes the lock,
//!   serves the same Control API on an ephemeral loopback port for the duration of the command
//!   and releases everything on exit (servers it started are stopped). Commands that start
//!   llama-server only do this with `--headless`.
//!
//! One code path for every command: they all talk to a Control API, so `--json` has the same
//! schema whichever way the command ran.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use harness_api::{ApiContext, ApiServer};
use harness_core::benchmark::BenchmarkManager;
use harness_core::control_api::ControlApiSettings;
use harness_core::downloader::DownloadManager;
use harness_core::instance::{self, AcquireError, InstanceKind, InstanceLock, OwnerState};
use harness_core::operation_gate::OperationGate;
use harness_core::runtime::RuntimeManager;
use harness_core::servers::ServerRegistry;
use harness_core::telemetry::TelemetryPoller;
use serde_json::{json, Value};

use crate::cli::GlobalArgs;
use crate::client::{normalize_base, ApiClient};
use crate::error::{CliError, CliResult, Exit};
use crate::output::Out;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The GUI's data directory (`tauri` `app_data_dir()`: the platform data dir + identifier).
pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("com.harnessstudio.app"))
}

pub fn resolve_data_dir(g: &GlobalArgs) -> CliResult<PathBuf> {
    let dir = match &g.data_dir {
        Some(d) if d.is_absolute() => d.clone(),
        Some(d) => std::env::current_dir()
            .map_err(|e| CliError::general("cli.io", e.to_string()))?
            .join(d),
        None => default_data_dir().ok_or_else(|| {
            CliError::general(
                "cli.io",
                "No per-user data directory on this system; pass --data-dir",
            )
        })?,
    };
    Ok(dir)
}

/// What a command needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// Library / profile / token work: a temporary instance is fine.
    Library,
    /// Starts llama-server: a running instance, or a temporary one only with `--headless`.
    Servers,
    /// Works on what a running instance runs (stop, logs, shutdown): never a temporary one.
    Existing,
}

/// How the command reaches its Control API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Owner,
    Remote,
    Temporary,
}

pub struct Session {
    pub client: ApiClient,
    pub via: Via,
    temp: Option<Instance>,
}

impl Session {
    /// Ends the session: a temporary instance stops its servers and API and releases the lock.
    pub async fn close(mut self) {
        if let Some(t) = self.temp.take() {
            t.shutdown().await;
        }
    }

    /// Replaces the host of a llama-server endpoint when the instance is remote (the API
    /// reports `127.0.0.1`-style connect hosts that only work on its own machine).
    pub fn reachable_endpoint(&self, endpoint: &str) -> String {
        if self.via != Via::Remote {
            return endpoint.to_string();
        }
        let api_host = self
            .client
            .origin()
            .split("://")
            .nth(1)
            .and_then(|a| a.rsplit_once(':').map(|(h, _)| h.to_string()));
        let (Some(api_host), Some((scheme, rest))) = (api_host, endpoint.split_once("://")) else {
            return endpoint.to_string();
        };
        let Some((authority, path)) = rest.split_once('/') else {
            return endpoint.to_string();
        };
        let Some((host, port)) = authority.rsplit_once(':') else {
            return endpoint.to_string();
        };
        if matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
            format!("{}://{}:{}/{}", scheme, api_host, port, path)
        } else {
            endpoint.to_string()
        }
    }
}

/// Reads the session token file an owner advertises.
fn read_token(path: &Path) -> CliResult<String> {
    std::fs::read_to_string(path)
        .map(|t| t.trim().to_string())
        .map_err(|e| {
            CliError::new(
                Exit::Connection,
                "cli.connection_failed",
                format!(
                    "The running instance's token file {} is not readable ({})",
                    path.display(),
                    e
                ),
            )
        })
}

/// The owner's API (if it runs one).
pub fn owner_client(info: &instance::InstanceInfo) -> CliResult<Option<ApiClient>> {
    let (Some(api), Some(token_path)) = (&info.api, &info.token_path) else {
        return Ok(None);
    };
    let host = if api.host.contains(':') {
        format!("[{}]", api.host)
    } else {
        api.host.clone()
    };
    // A wildcard bind is reachable on loopback.
    let host = match host.as_str() {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "[::]" => "[::1]".to_string(),
        _ => host,
    };
    let base = format!(
        "{}://{}:{}{}",
        if api.tls { "https" } else { "http" },
        host,
        api.port,
        harness_core::control_api::BASE_PATH
    );
    Ok(Some(ApiClient::new(base, read_token(token_path)?)))
}

/// The API of an instance that already runs (`--host`, or a live owner with its API on), or
/// `None` when there is none. Never starts anything: for commands that can also work locally.
pub fn existing_client(g: &GlobalArgs, data_dir: &Path) -> CliResult<Option<ApiClient>> {
    if let Some(host) = &g.host {
        let token = g.token.clone().filter(|t| !t.trim().is_empty());
        return token
            .map(|t| normalize_base(host).map(|b| ApiClient::new(b, t)))
            .transpose();
    }
    match instance::probe(data_dir) {
        OwnerState::Live(Some(info)) => owner_client(&info),
        _ => Ok(None),
    }
}

fn owner_without_api(info: &instance::InstanceInfo) -> CliError {
    CliError::from_code(Exit::Conflict, "cli.owner_without_api", Default::default())
        .with("pid", info.pid)
        .with(
            "kind",
            serde_json::to_value(info.kind).unwrap_or(Value::Null),
        )
}

fn lock_held(owner: Option<&instance::InstanceInfo>) -> CliError {
    let mut e = CliError::from_code(Exit::Conflict, "cli.lock_held", Default::default());
    if let Some(o) = owner {
        e = e.with("pid", o.pid);
        e.message = format!("{} (pid {})", e.message.trim_end_matches('.'), o.pid);
    }
    e
}

/// Finds or creates the Control API a command talks to.
pub async fn connect(g: &GlobalArgs, data_dir: &Path, need: Need, out: &Out) -> CliResult<Session> {
    if let Some(host) = &g.host {
        let token = g
            .token
            .clone()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| {
                CliError::usage(
                    "cli.invalid_argument",
                    "--host needs a token (--token or HARNESS_TOKEN)",
                )
            })?;
        return Ok(Session {
            client: ApiClient::new(normalize_base(host)?, token),
            via: Via::Remote,
            temp: None,
        });
    }
    match instance::probe(data_dir) {
        OwnerState::Live(Some(info)) => {
            let client = owner_client(&info)?.ok_or_else(|| owner_without_api(&info))?;
            Ok(Session {
                client,
                via: Via::Owner,
                temp: None,
            })
        }
        OwnerState::Live(None) => Err(lock_held(None)),
        OwnerState::None | OwnerState::Stale(_) => {
            if need == Need::Existing || (need == Need::Servers && !g.headless) {
                return Err(CliError::from_code(
                    Exit::Connection,
                    "cli.no_instance",
                    Default::default(),
                ));
            }
            if need == Need::Servers {
                out.info("Starting a temporary headless instance (servers stop when this command exits)...");
            }
            match Instance::start(data_dir, InstanceOptions::temporary()).await {
                Ok(temp) => Ok(Session {
                    client: temp.client(),
                    via: Via::Temporary,
                    temp: Some(temp),
                }),
                // Another process became the owner in the meantime: use its API if it has one.
                Err(e) if e.code == "cli.lock_held" => match instance::probe(data_dir) {
                    OwnerState::Live(Some(info)) => Ok(Session {
                        client: owner_client(&info)?.ok_or_else(|| owner_without_api(&info))?,
                        via: Via::Owner,
                        temp: None,
                    }),
                    _ => Err(e),
                },
                Err(e) => Err(e),
            }
        }
    }
}

/// Settings of an in-process instance.
pub struct InstanceOptions {
    pub browser: Option<harness_api::browser::BrowserConfig>,
    pub settings: ControlApiSettings,
    /// Pick a free loopback port (temporary instances).
    pub ephemeral_port: bool,
    /// The port was given explicitly (`--port`): fail when it is taken instead of falling back
    /// to another port.
    pub strict_port: bool,
    pub shutdown: Option<Arc<tokio::sync::Notify>>,
}

impl InstanceOptions {
    fn temporary() -> Self {
        Self {
            settings: ControlApiSettings {
                enabled: true,
                ..Default::default()
            },
            browser: None,
            ephemeral_port: true,
            strict_port: false,
            shutdown: None,
        }
    }
}

/// Core services + Control API of an in-process instance that owns the data directory
/// (temporary instances and `harness serve`).
pub struct Instance {
    pub setup_code: Option<String>,
    pub lock: Arc<InstanceLock>,
    pub api: ApiServer,
    pub ctx: ApiContext,
    pub base_url: String,
    token: String,
}

impl Instance {
    /// Takes the lock and starts the API. The data root must already be initialized
    /// (`harness_core::paths::init`, done in `main`).
    pub async fn start(data_dir: &Path, options: InstanceOptions) -> CliResult<Self> {
        let lock = match InstanceLock::acquire(data_dir, InstanceKind::Headless, VERSION) {
            Ok(lock) => Arc::new(lock),
            Err(AcquireError::Held { owner }) => return Err(lock_held(owner.as_ref())),
            Err(AcquireError::Io(e)) => return Err(CliError::general("cli.io", e)),
        };
        let restoring = data_dir.join("restore-pending.json").exists();
        if let Some(error) = harness_api::operations::apply_pending_or_quarantine(data_dir) {
            // Never block startup: a crash loop under systemd would hide the admin UI.
            eprintln!("staged restore failed, previous configuration kept: {error}");
        }
        if restoring {
            // serve reads settings before it can take the owner lock. Refresh its shared
            // cache after restoring, while keeping this listener's explicit bind options.
            let restored =
                harness_core::store::JsonStore::sensitive(data_dir.join("config/settings.json"))
                    .load_versioned::<harness_core::config::AppSettings>(
                        &harness_core::config::SETTINGS_SCHEMA,
                    )
                    .map_err(CliError::core)?;
            if let Some(restored) = restored {
                restored.data.save().map_err(CliError::core)?;
            }
        }
        let ctx = ApiContext {
            registry: Arc::new(ServerRegistry::default()),
            runtime: Arc::new(RuntimeManager::default()),
            downloads: Arc::new(DownloadManager::with_runtime(
                tokio::runtime::Handle::current(),
            )),
            benchmarks: Arc::new(BenchmarkManager::default()),
            gate: Arc::new(OperationGate::default()),
            telemetry: Arc::new(tokio::sync::Mutex::new(TelemetryPoller::with_adapters(
                harness_core::gpu::list_adapters(),
            ))),
            kind: InstanceKind::Headless,
            version: VERSION.to_string(),
            data_dir: data_dir.to_path_buf(),
        };
        // The persisted download queue must be loaded before anything is queued, or the
        // first save would drop it. Jobs come back paused.
        let downloads = ctx.downloads.clone();
        tokio::task::spawn_blocking(move || downloads.restore())
            .await
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
        let api = ApiServer::new();
        let setup_code = if let Some(config) = options.browser {
            let (auth, code) = harness_api::browser::BrowserAuth::open(data_dir, config)
                .map_err(CliError::core)?;
            api.set_browser(auth);
            code
        } else {
            None
        };
        api.set_instance(Some(lock.clone()));
        api.set_shutdown_hook(options.shutdown.clone());
        api.set_strict_port(options.strict_port);
        let mut settings = options.settings.clone();
        let started = if options.ephemeral_port {
            // Bound here and handed over: no other process can take the port in between.
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .map_err(|e| CliError::general("cli.io", e.to_string()))?;
            settings.host = "127.0.0.1".into();
            settings.port = listener
                .local_addr()
                .map_err(|e| CliError::general("cli.io", e.to_string()))?
                .port();
            api.start_on(ctx.clone(), &settings, listener).await
        } else {
            api.start(ctx.clone(), &settings).await
        };
        let status = started.map_err(|e| {
            let mut err = CliError::core(e);
            if err.code == "api.bind_failed" {
                err.exit = Exit::Conflict;
            }
            err
        })?;
        if let Some(fb) = &status.port_fallback {
            eprintln!(
                "warning: port {} is in use ({}); the Control API runs on port {}",
                fb.configured,
                fb.holder
                    .as_ref()
                    .map(|h| h.label())
                    .unwrap_or_else(|| "another program".into()),
                status.port.unwrap_or_default()
            );
        }
        let token = api.session_token().await.unwrap_or_default();
        Ok(Self {
            setup_code,
            lock,
            base_url: status.base_url.unwrap_or_default(),
            api,
            ctx,
            token,
        })
    }

    pub fn client(&self) -> ApiClient {
        ApiClient::new(self.base_url.clone(), self.token.clone())
    }

    /// Stops every llama-server of this instance, the API, and releases the data directory.
    pub async fn shutdown(self) {
        self.ctx.registry.stop_all().await;
        self.api.stop().await;
        harness_core::mcp::remove_all_runtime_configs();
        self.lock.release();
    }

    pub fn describe(&self) -> Value {
        let info = self.lock.info();
        json!({
            "pid": info.pid,
            "kind": info.kind,
            "base_url": self.base_url,
            "token_path": info.token_path,
            "data_dir": self.lock.dir(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(base: &str) -> Session {
        Session {
            client: ApiClient::new(normalize_base(base).unwrap(), "t".into()),
            via: Via::Remote,
            temp: None,
        }
    }

    #[test]
    fn remote_endpoints_use_the_api_host() {
        let s = remote("box.lan:11500");
        assert_eq!(
            s.reachable_endpoint("http://127.0.0.1:8081/v1"),
            "http://box.lan:8081/v1"
        );
        assert_eq!(
            s.reachable_endpoint("http://192.168.1.5:8081/v1"),
            "http://192.168.1.5:8081/v1"
        );
    }
}
