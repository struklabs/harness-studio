//! `harness doctor` / Settings > Diagnostics: one read-only report on whether this
//! installation can run llama-server.
//!
//! Checks (ids are stable): `data_dir`, `instance`, `runtime`, `runtime_compat`,
//! `gpu_libraries`, `gpu_devices`, `hardware`, `ports`. Each has a status (`ok` / `warn` /
//! `fail`), an English `summary` (CLI), an i18n `message_key` + `params` (GUI) and a
//! machine-readable `detail`.
//!
//! Nothing here writes state: no lock, no capability cache on disk; the only file touched is
//! a probe file created and removed in the data directory to test that it is writable.

use crate::capabilities;
use crate::config::{AppSettings, RuntimeMode};
use crate::instance::{self, OwnerState};
use crate::runtime::RuntimeManager;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Check {
    pub id: String,
    pub status: CheckStatus,
    /// English one-liner (CLI).
    pub summary: String,
    /// i18n key (`doctor.*`) of the same message (GUI).
    pub message_key: String,
    pub params: BTreeMap<String, String>,
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub detail: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DoctorReport {
    /// No check failed.
    pub ok: bool,
    #[cfg_attr(feature = "openapi", schema(value_type = String))]
    pub data_dir: PathBuf,
    pub version: String,
    pub checks: Vec<Check>,
    /// The runtime the next start would use (same as the `runtime` check's detail).
    #[cfg_attr(feature = "openapi", schema(value_type = Object))]
    pub runtime: Value,
}

struct C {
    id: &'static str,
    status: CheckStatus,
    summary: String,
    key: &'static str,
    params: Vec<(&'static str, String)>,
    detail: Value,
}

impl C {
    fn build(self) -> Check {
        Check {
            id: self.id.to_string(),
            status: self.status,
            summary: self.summary,
            message_key: format!("doctor.{}", self.key),
            params: self
                .params
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
            detail: self.detail,
        }
    }
}

/// Decimal units like the CLI (`12.3 GB`).
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit + 1 < UNITS.len() {
        v /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", n)
    } else {
        format!("{:.1} {}", v, UNITS[unit])
    }
}

pub fn owner_json(state: &OwnerState) -> Value {
    match state {
        OwnerState::None => json!({"state": "none"}),
        OwnerState::Live(Some(info)) => json!({
            "state": if info.api.is_some() { "running" } else { "running_without_api" },
            "info": info,
        }),
        OwnerState::Live(None) => json!({"state": "locked"}),
        OwnerState::Stale(info) => json!({"state": "stale", "info": info}),
    }
}

/// Runtime binary the next start would use (read-only; the probe is not cached to disk).
pub fn runtime_json() -> Value {
    let settings = AppSettings::load();
    let mode = match settings.runtime_mode {
        RuntimeMode::Managed => "managed",
        RuntimeMode::Custom => "custom",
    };
    match crate::orchestrator::resolve_server_binary_detail(&RuntimeManager::default(), &settings) {
        Ok(resolved) => {
            let binary = resolved.path;
            // The official runtime's engine: a cached probe, else a fresh one that is not saved.
            let engine = crate::runtimes::engine_for(None).unwrap_or(crate::engine::llama_cpp());
            let caps = engine
                .cached_capabilities(&binary)
                .unwrap_or_else(|| std::sync::Arc::new(engine.probe_capabilities(&binary)));
            json!({
                "mode": mode,
                // Where the binary really comes from (`managed`, `custom`, `system`) and whether
                // that is a fallback because the configured mode can't be used.
                "source": resolved.source,
                "fallback": resolved.fallback,
                "binary_path": binary,
                "version": caps.version,
                "help_parsed": caps.help_parsed,
                "flag_count": caps.flags.len(),
                "devices": caps.devices,
                "error": Value::Null,
            })
        }
        Err(e) => json!({"mode": mode, "binary_path": Value::Null, "error": e, "devices": []}),
    }
}

/// Same check as a server port: a connection first, then a bind. A bind alone misses another
/// program's wildcard listener (`[::]`, `0.0.0.0`) on macOS and Windows, where binding
/// 127.0.0.1 next to it succeeds.
fn port_in_use(host: &str, port: u16) -> bool {
    crate::servers::ensure_port_free(host, port).is_err()
}

fn data_dir_check(data_dir: &Path) -> C {
    if !data_dir.exists() {
        return C {
            id: "data_dir",
            status: CheckStatus::Warn,
            summary: format!(
                "{} does not exist yet (created on first use)",
                data_dir.display()
            ),
            key: "data_dir_missing",
            params: vec![("path", data_dir.display().to_string())],
            detail: json!({"path": data_dir}),
        };
    }
    // Writable: create and remove a probe file (not a state file).
    let probe = data_dir.join(format!(".doctor-{}", std::process::id()));
    let writable = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(&probe);
    let mut detail = json!({"path": data_dir, "writable": writable});
    let path = data_dir.display().to_string();
    if !writable {
        return C {
            id: "data_dir",
            status: CheckStatus::Fail,
            summary: format!("{} is not writable", path),
            key: "data_dir_not_writable",
            params: vec![("path", path)],
            detail,
        };
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(data_dir) {
            let mode = meta.permissions().mode() & 0o777;
            detail["mode"] = json!(format!("{:o}", mode));
            let group_is_private = {
                use std::os::unix::fs::MetadataExt;
                user_private_group(meta.uid(), meta.gid())
            };
            let shared = mode & 0o002 != 0 || (mode & 0o020 != 0 && !group_is_private);
            if shared {
                return C {
                    id: "data_dir",
                    status: CheckStatus::Warn,
                    summary: format!("writable by other users (mode {:o})", mode),
                    key: "data_dir_shared",
                    params: vec![("path", path), ("mode", format!("{:o}", mode))],
                    detail,
                };
            }
        }
    }
    let token = crate::control_api::session_token_path(data_dir);
    if token.exists() {
        match instance::is_private_to_current_user(&token) {
            Ok(true) => detail["token_file_private"] = json!(true),
            Ok(false) => {
                detail["token_file_private"] = json!(false);
                return C {
                    id: "data_dir",
                    status: CheckStatus::Warn,
                    summary: "the session token file is readable by others".into(),
                    key: "token_file_shared",
                    params: vec![("path", token.display().to_string())],
                    detail,
                };
            }
            Err(e) => detail["token_file_error"] = json!(e.to_string()),
        }
    }
    C {
        id: "data_dir",
        status: CheckStatus::Ok,
        summary: format!("{} is writable", path),
        key: "data_dir_ok",
        params: vec![("path", path)],
        detail,
    }
}

fn instance_check(state: &OwnerState) -> C {
    let detail = owner_json(state);
    let (status, summary, key, params) = match state {
        OwnerState::None => (
            CheckStatus::Ok,
            "no instance running".to_string(),
            "instance_none",
            vec![],
        ),
        OwnerState::Stale(i) => (
            CheckStatus::Ok,
            format!("stale instance.json of pid {} (harmless)", i.pid),
            "instance_stale",
            vec![("pid", i.pid.to_string())],
        ),
        OwnerState::Live(Some(i)) => {
            let kind = format!("{:?}", i.kind).to_lowercase();
            let pid = i.pid.to_string();
            if i.api.is_some() {
                (
                    CheckStatus::Ok,
                    format!("{} instance, pid {}, Control API on", kind, pid),
                    "instance_running",
                    vec![("kind", kind), ("pid", pid)],
                )
            } else {
                (
                    CheckStatus::Warn,
                    format!(
                        "{} instance, pid {}, Control API off (the CLI cannot reach it)",
                        kind, pid
                    ),
                    "instance_no_api",
                    vec![("kind", kind), ("pid", pid)],
                )
            }
        }
        OwnerState::Live(None) => (
            CheckStatus::Warn,
            "the data directory is locked by another process".to_string(),
            "instance_locked",
            vec![],
        ),
    };
    C {
        id: "instance",
        status,
        summary,
        key,
        params,
        detail,
    }
}

fn runtime_check(runtime: &Value) -> C {
    let path = runtime["binary_path"].as_str().map(str::to_string);
    match path {
        Some(path) => {
            let parsed = runtime["help_parsed"] == true;
            let version = runtime["version"]["raw"]
                .as_str()
                .unwrap_or("version unknown")
                .to_string();
            C {
                id: "runtime",
                status: if parsed {
                    CheckStatus::Ok
                } else {
                    CheckStatus::Warn
                },
                summary: format!("{} ({})", path, version),
                key: if parsed {
                    "runtime_ok"
                } else {
                    "runtime_unparsed"
                },
                params: vec![("path", path), ("version", version)],
                detail: runtime.clone(),
            }
        }
        None => {
            let error = runtime["error"].as_str().unwrap_or_default().to_string();
            C {
                id: "runtime",
                status: CheckStatus::Fail,
                summary: format!("no llama-server: {}", error),
                key: "runtime_missing",
                params: vec![("error", error)],
                detail: runtime.clone(),
            }
        }
    }
}

/// Build number in a version tag (`b11236` → 11236).
fn build_of(tag: &str) -> Option<u64> {
    let digits: String = tag
        .trim()
        .trim_start_matches(['b', 'B'])
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// `--version` of a binary (build number), quick (no `--help`, no device probe).
fn quick_build(binary: &Path) -> Option<u64> {
    let mut cmd = crate::process::quiet_command(binary);
    cmd.arg("--version")
        .current_dir(binary.parent().unwrap_or_else(|| Path::new(".")));
    let out = crate::runtime::run_command_with_timeout(
        &mut cmd,
        crate::runtime::PROBE_TIMEOUT,
        crate::runtime::PROBE_MAX_BYTES,
    )
    .ok()?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    capabilities::parse::parse_version(&text).build
}

/// Recorded managed version vs the binary, and a staged update that was never activated.
fn runtime_compat_check(runtime: &Value, settings: &AppSettings, rt: &RuntimeManager) -> C {
    let actual = runtime["version"]["build"].as_u64();
    let mode = runtime["mode"].as_str().unwrap_or_default().to_string();
    let recorded = settings.managed_runtime.installed_version.clone();
    let staged_bin = rt.staging_dir().join(crate::runtime::binary_name());
    let staged = staged_bin
        .is_file()
        .then(|| quick_build(&staged_bin))
        .flatten();
    let detail = json!({
        "mode": mode,
        "build": actual,
        "recorded_version": recorded,
        "staged_build": staged,
        "staged_path": staged_bin.is_file().then_some(&staged_bin),
    });
    let mk = |status, summary: String, key, params| C {
        id: "runtime_compat",
        status,
        summary,
        key,
        params,
        detail: detail.clone(),
    };
    let Some(actual) = actual else {
        return mk(
            CheckStatus::Warn,
            "runtime build number unknown (custom build?)".into(),
            "compat_unknown",
            vec![],
        );
    };
    if mode == "managed" {
        if let Some(rec) = recorded.as_deref().and_then(build_of) {
            if rec != actual {
                return mk(
                    CheckStatus::Warn,
                    format!(
                        "settings record b{} but the runtime binary is b{}",
                        rec, actual
                    ),
                    "compat_mismatch",
                    vec![
                        ("recorded", format!("b{rec}")),
                        ("actual", format!("b{actual}")),
                    ],
                );
            }
        }
    }
    if runtime["fallback"] == true {
        let path = runtime["binary_path"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let source = runtime["source"].as_str().unwrap_or_default().to_string();
        return mk(
            CheckStatus::Warn,
            format!(
                "runtime mode is {} but that runtime is not available; the {} llama-server b{} is used instead ({})",
                mode, source, actual, path
            ),
            "compat_fallback",
            vec![
                ("mode", mode.clone()),
                ("source", source),
                ("actual", format!("b{actual}")),
                ("path", path),
            ],
        );
    }
    if staged_bin.is_file() {
        let staged_text = staged
            .map(|b| format!("b{b}"))
            .unwrap_or_else(|| "?".into());
        return mk(
            CheckStatus::Warn,
            format!(
                "a runtime ({}) is staged but not activated; b{} is in use",
                staged_text, actual
            ),
            "compat_staged",
            vec![("staged", staged_text), ("actual", format!("b{actual}"))],
        );
    }
    mk(
        CheckStatus::Ok,
        format!("b{} ({})", actual, mode),
        "compat_ok",
        vec![("actual", format!("b{actual}")), ("mode", mode.clone())],
    )
}

/// GPU driver libraries of this OS: `(backend, library, found path)`.
fn driver_libraries() -> Vec<(&'static str, String, Option<PathBuf>)> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("System32");
        let exact = |name: &str| {
            let p = system.join(name);
            p.is_file().then_some(p)
        };
        out.push(("vulkan", "vulkan-1.dll".to_string(), exact("vulkan-1.dll")));
        out.push(("cuda", "nvcuda.dll".to_string(), exact("nvcuda.dll")));
        // HIP runtime: amdhip64.dll / amdhip64_6.dll / amdhip64_7.dll.
        let hip = std::fs::read_dir(&system).ok().and_then(|rd| {
            rd.flatten().map(|e| e.path()).find(|p| {
                let n = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                n.starts_with("amdhip64") && n.ends_with(".dll")
            })
        });
        out.push(("rocm", "amdhip64*.dll".to_string(), hip));
    }
    #[cfg(target_os = "linux")]
    {
        let ldconfig = crate::process::hidden_command("ldconfig")
            .arg("-p")
            .stdin(std::process::Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .unwrap_or_default();
        let dirs = [
            "/usr/lib",
            "/usr/lib64",
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
            "/usr/lib/wsl/lib",
            "/opt/rocm/lib",
            "/usr/local/cuda/lib64",
        ];
        let find = |name: &str| -> Option<PathBuf> {
            if let Some(line) = ldconfig.lines().find(|l| l.trim_start().starts_with(name)) {
                if let Some((_, path)) = line.split_once("=>") {
                    return Some(PathBuf::from(path.trim()));
                }
            }
            dirs.iter()
                .map(|d| Path::new(d).join(name))
                .find(|p| p.exists())
        };
        out.push(("vulkan", "libvulkan.so.1".into(), find("libvulkan.so.1")));
        out.push(("cuda", "libcuda.so.1".into(), find("libcuda.so.1")));
        out.push(("rocm", "libamdhip64.so".into(), find("libamdhip64.so")));
    }
    #[cfg(target_os = "macos")]
    {
        let metal = PathBuf::from("/System/Library/Frameworks/Metal.framework");
        out.push((
            "metal",
            "Metal.framework".into(),
            metal.exists().then_some(metal),
        ));
    }
    out
}

/// GPU backends a runtime directory ships (`ggml-vulkan.dll`, `libggml-cuda.so`...).
fn runtime_backends(binary: &Path) -> Vec<&'static str> {
    let Some(dir) = binary.parent() else {
        return Vec::new();
    };
    let names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_lowercase())
                .collect()
        })
        .unwrap_or_default();
    let has = |needle: &str| names.iter().any(|n| n.contains(needle));
    let mut out = Vec::new();
    for (backend, needle) in [
        ("vulkan", "ggml-vulkan"),
        ("cuda", "ggml-cuda"),
        ("rocm", "ggml-hip"),
        ("metal", "ggml-metal"),
        ("sycl", "ggml-sycl"),
    ] {
        if has(needle) {
            out.push(backend);
        }
    }
    out
}

fn gpu_libraries_check(runtime: &Value) -> C {
    let libs = driver_libraries();
    let binary = runtime["binary_path"].as_str().map(PathBuf::from);
    let backends = binary.as_deref().map(runtime_backends).unwrap_or_default();
    let detail = json!({
        "libraries": libs.iter().map(|(b, n, p)| json!({"backend": b, "library": n, "path": p, "found": p.is_some()})).collect::<Vec<_>>(),
        "runtime_backends": backends,
    });
    let found: Vec<&str> = libs
        .iter()
        .filter(|(_, _, p)| p.is_some())
        .map(|(b, _, _)| *b)
        .collect();
    // A backend the runtime ships whose driver library is missing cannot use the GPU.
    let missing: Vec<(&str, String)> = libs
        .iter()
        .filter(|(b, _, p)| p.is_none() && backends.contains(b))
        .map(|(b, n, _)| (*b, n.clone()))
        .collect();
    let mut usable: Vec<&str> = backends
        .iter()
        .copied()
        .filter(|b| found.contains(b) || *b == "metal" && cfg!(target_os = "macos"))
        .collect();
    usable.dedup();
    let status_for = |s, summary: String, key, params| C {
        id: "gpu_libraries",
        status: s,
        summary,
        key,
        params,
        detail: detail.clone(),
    };
    if !missing.is_empty() && usable.is_empty() {
        let (backend, library) = missing[0].clone();
        return status_for(
            CheckStatus::Fail,
            format!(
                "the runtime uses {} but {} is missing (GPU driver not installed?)",
                backend, library
            ),
            "gpu_libs_missing",
            vec![("backend", backend.to_string()), ("library", library)],
        );
    }
    if found.is_empty() && libs.iter().any(|(b, _, _)| *b != "metal") {
        return status_for(
            CheckStatus::Warn,
            "no GPU driver library found (Vulkan, CUDA, ROCm): CPU only".into(),
            "gpu_libs_none",
            vec![],
        );
    }
    status_for(
        CheckStatus::Ok,
        format!(
            "driver libraries: {}{}",
            found.join(", "),
            if backends.is_empty() {
                String::new()
            } else {
                format!("; runtime backends: {}", backends.join(", "))
            }
        ),
        "gpu_libs_ok",
        vec![
            ("found", found.join(", ")),
            ("backends", backends.join(", ")),
        ],
    )
}

fn devices_check(devices: &Value) -> C {
    let list = devices.as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        return C {
            id: "gpu_devices",
            status: CheckStatus::Warn,
            summary: "llama-server --list-devices reports no GPU (CPU only)".into(),
            key: "devices_none",
            params: vec![],
            detail: devices.clone(),
        };
    }
    let text = list
        .iter()
        .map(|d| {
            format!(
                "{} {}{}",
                d["id"].as_str().unwrap_or_default(),
                d["name"].as_str().unwrap_or_default(),
                d["total_mib"]
                    .as_u64()
                    .map(|m| format!(" ({} MiB)", m))
                    .unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    C {
        id: "gpu_devices",
        status: CheckStatus::Ok,
        summary: text.clone(),
        key: "devices_ok",
        params: vec![("devices", text)],
        detail: devices.clone(),
    }
}

fn hardware_check() -> C {
    let hw = crate::hardware::detect_hardware();
    let cpu = hw.cpu_name.trim().to_string();
    C {
        id: "hardware",
        status: CheckStatus::Ok,
        summary: format!(
            "{} ({} cores), {} RAM, {} GPU(s)",
            cpu,
            hw.cpu_cores,
            fmt_bytes(hw.total_ram_bytes),
            hw.gpus.len()
        ),
        key: "hardware",
        params: vec![
            ("cpu", cpu.clone()),
            ("cores", hw.cpu_cores.to_string()),
            ("ram", fmt_bytes(hw.total_ram_bytes)),
            ("gpus", hw.gpus.len().to_string()),
        ],
        detail: serde_json::to_value(&hw).unwrap_or(Value::Null),
    }
}

/// Ports: the Control API, the default server port and every profile's port. A port in use
/// is only a problem when nothing of ours holds it; two profiles on one port always are.
fn ports_check(state: &OwnerState, settings: &AppSettings, own_ports: &[u16]) -> C {
    let running = matches!(state, OwnerState::Live(_));
    let mut owned: Vec<u16> = own_ports.to_vec();
    if let OwnerState::Live(Some(info)) = state {
        owned.extend(info.api.iter().map(|a| a.port));
    }
    let mut ports: Vec<(String, String, u16)> = vec![
        (
            "control_api".into(),
            settings.control_api.host.clone(),
            settings.control_api.port,
        ),
        (
            "default_server".into(),
            settings.server.host.clone(),
            settings.server.port,
        ),
    ];
    // Read the profile file as it is (the domain store could import legacy files, a write).
    let profiles: Value =
        std::fs::read_to_string(crate::paths::config_dir().join("server_profiles.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
    for p in profiles["profiles"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
    {
        // Automatic ports move on their own when taken.
        if p["port_auto"].as_bool() == Some(true) {
            continue;
        }
        if let (Some(id), Some(port)) = (p["id"].as_str(), p["port"].as_u64()) {
            let host = p["host"].as_str().unwrap_or("127.0.0.1").to_string();
            ports.push((format!("profile:{}", id), host, port as u16));
        }
    }
    let mut busy = Vec::new();
    let mut detail = Vec::new();
    ports.retain(|(_, _, port)| *port != 0);
    for (what, host, port) in &ports {
        let in_use = port_in_use(host, *port);
        detail.push(json!({"use": what, "host": host, "port": port, "in_use": in_use}));
        if in_use && !owned.contains(port) {
            // Name the program holding it when the OS tells us ("11500 (python3, pid 812)").
            busy.push(match crate::ports::holder(*port) {
                Some(h) if !h.name.is_empty() => {
                    format!("{} {}:{} ({}, pid {})", what, host, port, h.name, h.pid)
                }
                Some(h) => format!("{} {}:{} (pid {})", what, host, port, h.pid),
                None => format!("{} {}:{}", what, host, port),
            });
        }
    }
    let mut dup = Vec::new();
    for (i, (a, _, pa)) in ports.iter().enumerate() {
        for (b, _, pb) in ports.iter().skip(i + 1) {
            if pa == pb && a.starts_with("profile:") && b.starts_with("profile:") {
                dup.push(format!("{} = {} ({})", a, b, pa));
            }
        }
    }
    let detail = json!(detail);
    if !dup.is_empty() {
        return C {
            id: "ports",
            status: CheckStatus::Warn,
            summary: format!("profiles share a port: {}", dup.join(", ")),
            key: "ports_duplicate",
            params: vec![("ports", dup.join(", "))],
            detail,
        };
    }
    if busy.is_empty() {
        return C {
            id: "ports",
            status: CheckStatus::Ok,
            summary: "configured ports are free".into(),
            key: "ports_ok",
            params: vec![],
            detail,
        };
    }
    let list = busy.join(", ");
    C {
        id: "ports",
        status: CheckStatus::Warn,
        summary: if running {
            format!(
                "in use (possibly by the running instance's servers): {}",
                list
            )
        } else {
            format!("in use by another program: {}", list)
        },
        key: if running {
            "ports_busy_running"
        } else {
            "ports_busy"
        },
        params: vec![("ports", list)],
        detail,
    }
}

/// Runs every check. `own_ports`: ports the calling process's own servers hold (the GUI
/// passes its running servers; the CLI has none).
pub fn run(data_dir: &Path, version: &str, own_ports: &[u16]) -> DoctorReport {
    let settings = AppSettings::load();
    let rt = RuntimeManager::default();
    let state = instance::probe(data_dir);
    let runtime = runtime_json();
    let mut checks = vec![
        data_dir_check(data_dir),
        instance_check(&state),
        runtime_check(&runtime),
    ];
    if runtime["binary_path"].is_string() {
        checks.push(runtime_compat_check(&runtime, &settings, &rt));
    }
    checks.push(gpu_libraries_check(&runtime));
    if runtime["binary_path"].is_string() {
        checks.push(devices_check(&runtime["devices"]));
    }
    checks.push(hardware_check());
    checks.push(ports_check(&state, &settings, own_ports));
    let checks: Vec<Check> = checks.into_iter().map(C::build).collect();
    DoctorReport {
        ok: !checks.iter().any(|c| c.status == CheckStatus::Fail),
        data_dir: data_dir.to_path_buf(),
        version: version.to_string(),
        checks,
        runtime,
    }
}

/// Every `message_key` a check can return (kept in step with the GUI locale by a test).
pub const MESSAGE_KEYS: &[&str] = &[
    "doctor.data_dir_missing",
    "doctor.data_dir_not_writable",
    "doctor.data_dir_shared",
    "doctor.token_file_shared",
    "doctor.data_dir_ok",
    "doctor.instance_none",
    "doctor.instance_stale",
    "doctor.instance_running",
    "doctor.instance_no_api",
    "doctor.instance_locked",
    "doctor.runtime_ok",
    "doctor.runtime_unparsed",
    "doctor.runtime_missing",
    "doctor.compat_unknown",
    "doctor.compat_mismatch",
    "doctor.compat_staged",
    "doctor.compat_fallback",
    "doctor.compat_ok",
    "doctor.gpu_libs_missing",
    "doctor.gpu_libs_none",
    "doctor.gpu_libs_ok",
    "doctor.devices_none",
    "doctor.devices_ok",
    "doctor.hardware",
    "doctor.ports_duplicate",
    "doctor.ports_ok",
    "doctor.ports_busy_running",
    "doctor.ports_busy",
];

/// True when `gid` is the user-private group of `uid` (Debian/Ubuntu/Fedora default, umask
/// 002): the user's primary group, named after the user, with no other members. Group write
/// on such a directory reaches nobody else, so it is not reported as shared.
#[cfg(unix)]
fn user_private_group(uid: u32, gid: u32) -> bool {
    use std::ffi::CStr;
    let mut buf = vec![0 as libc::c_char; 16 * 1024];
    // SAFETY: the `_r` variants write only into `pwd`/`grp` and `buf`, which outlive every read.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut out) != 0
            || out.is_null()
            || pwd.pw_gid != gid
        {
            return false;
        }
        let user = CStr::from_ptr(pwd.pw_name).to_bytes().to_vec();
        let mut gbuf = vec![0 as libc::c_char; 16 * 1024];
        let mut grp: libc::group = std::mem::zeroed();
        let mut gout: *mut libc::group = std::ptr::null_mut();
        if libc::getgrgid_r(gid, &mut grp, gbuf.as_mut_ptr(), gbuf.len(), &mut gout) != 0
            || gout.is_null()
        {
            return false;
        }
        if CStr::from_ptr(grp.gr_name).to_bytes() != user.as_slice() {
            return false;
        }
        let mut member = grp.gr_mem;
        while !member.is_null() && !(*member).is_null() {
            if CStr::from_ptr(*member).to_bytes() != user.as_slice() {
                return false;
            }
            member = member.add(1);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_numbers_of_tags() {
        assert_eq!(build_of("b11236"), Some(11236));
        assert_eq!(build_of("11026"), Some(11026));
        assert_eq!(build_of("latest"), None);
    }

    #[test]
    fn message_keys_are_in_both_locales() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/lib/locales");
        let text: String = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
            .collect();
        for key in MESSAGE_KEYS {
            assert_eq!(
                text.matches(&format!("'{}':", key)).count(),
                2,
                "{key} must be in the TR and EN locale"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn world_writable_data_dir_warns_but_user_private_group_write_does_not() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = tempfile::tempdir().unwrap();
        let set =
            |mode| std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode));
        set(0o777).unwrap();
        assert_eq!(
            run(dir.path(), "test", &[]).checks[0].status,
            CheckStatus::Warn
        );
        set(0o770).unwrap();
        let meta = std::fs::metadata(dir.path()).unwrap();
        let expected = if user_private_group(meta.uid(), meta.gid()) {
            CheckStatus::Ok
        } else {
            CheckStatus::Warn
        };
        assert_eq!(run(dir.path(), "test", &[]).checks[0].status, expected);
        set(0o700).unwrap();
        assert_eq!(
            run(dir.path(), "test", &[]).checks[0].status,
            CheckStatus::Ok
        );
    }

    // Not on Windows: a test binary listening on every interface opens a Windows Firewall
    // prompt on each new build, which also steals focus from UI automation. The regression
    // it guards is macOS-only (SO_REUSEADDR); Linux runs it too.
    #[cfg(not(windows))]
    #[test]
    fn a_wildcard_listener_counts_as_in_use() {
        // macOS lets 127.0.0.1 bind next to it (SO_REUSEADDR), so a bind alone said "free".
        let other = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let port = other.local_addr().unwrap().port();
        assert!(port_in_use("127.0.0.1", port));
    }

    #[test]
    fn report_on_a_temp_data_dir_has_every_check_and_stable_shape() {
        let dir = tempfile::tempdir().unwrap();
        let report = run(dir.path(), "test", &[]);
        let ids: Vec<&str> = report.checks.iter().map(|c| c.id.as_str()).collect();
        for id in [
            "data_dir",
            "instance",
            "runtime",
            "gpu_libraries",
            "hardware",
            "ports",
        ] {
            assert!(ids.contains(&id), "{id} in {ids:?}");
        }
        let dd = &report.checks[0];
        assert_eq!(dd.status, CheckStatus::Ok, "{dd:?}");
        assert_eq!(dd.message_key, "doctor.data_dir_ok");
        for c in &report.checks {
            assert!(
                MESSAGE_KEYS.contains(&c.message_key.as_str()),
                "{}",
                c.message_key
            );
            assert!(!c.summary.is_empty());
        }
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["checks"][0]["status"], "ok");
        assert!(json["checks"][0]["params"].is_object());
        assert_eq!(
            report.ok,
            !report.checks.iter().any(|c| c.status == CheckStatus::Fail)
        );
    }
}
