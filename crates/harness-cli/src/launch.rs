//! `harness launch <tool>`: start Claude Code, Codex, OpenCode, Aider or Goose on
//! a Harness model with process-only settings (`harness_core::integrations::launch`).
//!
//! 1. Model: `--model` (definition alias, model name or `.gguf`), else a picker on a terminal
//!    (or the one running definition).
//! 2. Endpoint: the managed endpoint while the gateway runs (default), `--direct` for the
//!    llama-server port.
//! 3. Context: the running server's per-slot context, else the definition's. Below the tool's
//!    minimum (or unknown) "prepare the server" writes the agent recommendation into the
//!    definition (context, flash attention, KV cache precision, automatic concurrent requests) and reloads — after a
//!    confirmation, `--prepare` or `--yes`.
//! 4. Nothing serves the model: the gateway loads it on a warm-up request (JIT); without the
//!    gateway the definition is loaded on a new server.
//! 5. The tool runs with our env/args (secrets only in its environment or a private temp file)
//!    in this terminal; `harness` waits and exits with the tool's exit code. `--dry-run` prints
//!    the plan with secrets redacted and starts nothing.

use std::io::{BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

use harness_core::integrations::launch::{self, CtxVerdict, LaunchInput, LaunchTool};
use harness_core::prepare::{self, OptionKind, PrepareOption, PreparePlan, Verdict};
use reqwest::Method;
use serde_json::{json, Value};

use crate::cli::LaunchArgs;
use crate::client::seg;
use crate::commands::resolve::{definition_for, idle_server_of, servers_of, wait_running};
use crate::error::{CliError, CliResult, Exit};
use crate::output::{str_of, Out};
use crate::session::Session;

/// Exit code of the launched tool (the process exits with it).
pub static CHILD_EXIT: AtomicI32 = AtomicI32::new(0);

/// How the tool reaches the model.
struct Endpoint {
    /// `http://host:port`
    host_url: String,
    token: Option<String>,
    managed: bool,
}

fn host_of(base_url: &str) -> String {
    let b = base_url.trim_end_matches('/');
    b.strip_suffix("/v1").unwrap_or(b).to_string()
}

/// A number from a definition's `load` (ints are stored as numbers or text).
fn load_u32(def: &Value, id: &str) -> Option<u32> {
    let v = &def["load"][id];
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0)
}

/// Per-request context of a running server: llama-server's own `/props` (`n_ctx` per slot),
/// else the launch config (context / slots when slots are set explicitly).
async fn running_ctx(session: &Session, view: &Value, api_key: Option<&str>) -> Option<u32> {
    if let Some(endpoint) = view["endpoint"].as_str() {
        let host = host_of(&session.reachable_endpoint(endpoint));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .no_proxy()
            .build()
            .ok()?;
        let mut req = http.get(format!("{}/props", host));
        if let Some(k) = api_key {
            req = req.bearer_auth(k);
        }
        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(props) = resp.json::<Value>().await {
                    let n = props["default_generation_settings"]["n_ctx"]
                        .as_u64()
                        .or_else(|| props["n_ctx"].as_u64());
                    if let Some(n) = n.and_then(|n| u32::try_from(n).ok()) {
                        return Some(n);
                    }
                }
            }
        }
    }
    let eff = session
        .client
        .get(&format!("/servers/{}/effective", seg(&str_of(&view["id"]))))
        .await
        .ok()?;
    let ctx = eff["config"]["context_size"].as_u64()? as u32;
    // Slots share one pool (`--kv-unified`, Harness's default): every request may use all of
    // it. Only an explicit `--no-kv-unified` splits it.
    let split = eff["config"]["params"]["kv_unified"].as_bool() == Some(false);
    let slots = eff["config"]["parallel_slots"].as_u64().unwrap_or(1).max(1) as u32;
    Some(if split { ctx / slots } else { ctx })
}

/// The definition to launch with: `--model`, else a picker (terminal) or the only running one.
async fn pick_definition(session: &Session, model: Option<&str>, out: &Out) -> CliResult<Value> {
    let client = &session.client;
    if let Some(m) = model {
        return definition_for(client, m, out).await;
    }
    let models = client.get("/models").await?;
    let defs: Vec<Value> = models["definitions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    if defs.is_empty() {
        return Err(CliError::new(
            Exit::NotFound,
            "cli.no_definitions",
            "No model definitions yet: pass --model with a model name or a .gguf file",
        ));
    }
    let servers = client.get("/servers").await?;
    let running: Vec<String> = servers
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["status"] == "Running")
        .filter_map(|s| s["definition_id"].as_str().map(str::to_string))
        .collect();
    let mut ordered: Vec<&Value> = defs.iter().collect();
    ordered.sort_by_key(|d| {
        (
            !running.contains(&str_of(&d["id"])),
            str_of(&d["alias"]).to_lowercase(),
        )
    });
    let interactive = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !interactive {
        let running_defs: Vec<&&Value> = ordered
            .iter()
            .filter(|d| running.contains(&str_of(&d["id"])))
            .collect();
        return match running_defs.as_slice() {
            [one] => Ok((**one).clone()),
            _ => Err(CliError::usage(
                "cli.invalid_argument",
                "Pass --model <alias> (no terminal to pick one)",
            )),
        };
    }
    eprintln!("Models:");
    for (i, d) in ordered.iter().enumerate() {
        let ctx = load_u32(d, "context_size")
            .map(|c| format!("ctx {}", c))
            .unwrap_or_default();
        let state = if running.contains(&str_of(&d["id"])) {
            "running"
        } else {
            ""
        };
        eprintln!(
            "  {:>2}) {:<32} {:<10} {}",
            i + 1,
            str_of(&d["alias"]),
            state,
            ctx
        );
    }
    eprint!("Model [1]: ");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    let pick = line.trim();
    let idx = if pick.is_empty() {
        0
    } else if let Ok(n) = pick.parse::<usize>() {
        n.checked_sub(1)
            .filter(|i| *i < ordered.len())
            .ok_or_else(|| CliError::usage("cli.invalid_argument", "No such entry"))?
    } else {
        return definition_for(client, pick, out).await;
    };
    Ok(ordered[idx].clone())
}

/// Sends a one-token chat request through the gateway so it loads the model (JIT).
async fn warm_up(endpoint: &Endpoint, alias: &str, out: &Out) -> CliResult<()> {
    out.info(format!("Loading {} through the gateway...", alias));
    let url = format!("{}/v1/chat/completions", endpoint.host_url);
    let http = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(7300))
        .no_proxy()
        .build()
        .map_err(|e| CliError::core(harness_core::http_error::transport(&e, &url)))?;
    let mut req = http
        .post(&url)
        .header("x-harness-client", "harness-launch")
        .json(&json!({
            "model": alias,
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 1,
        }));
    if let Some(t) = &endpoint.token {
        req = req.bearer_auth(t);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| CliError::transport(&e, &url))?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let code = resp
        .headers()
        .get("x-harness-error")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    let code = if code.is_empty() {
        body["error"]["code"].as_str().unwrap_or_default()
    } else {
        &code
    };
    let msg = body["error"]["message"]
        .as_str()
        .unwrap_or("the gateway could not load the model")
        .to_string();
    let exit = if code.contains("memory")
        || code == "oom"
        || crate::messages::mentions_out_of_memory(&msg)
    {
        Exit::OutOfMemory
    } else {
        crate::error::exit_for_status(status)
    };
    Err(CliError::new(exit, "cli.launch_load_failed", msg).with("http_status", status))
}

/// Writes the agent recommendation into the definition (`load`).
async fn prepare_definition(session: &Session, def: &Value, values: &Value) -> CliResult<Value> {
    let id = str_of(&def["id"]);
    let path = format!("/definitions/{}", seg(&id));
    let mut full = session.client.get(&path).await?;
    let previous = full["load"].clone();
    if !full["load"].is_object() {
        full["load"] = json!({});
    }
    for (k, v) in values.as_object().into_iter().flatten() {
        // `null` = back to automatic (the key is removed: `load` values are never null).
        if v.is_null() {
            if let Some(load) = full["load"].as_object_mut() {
                load.remove(k);
            }
        } else {
            full["load"][k] = v.clone();
        }
    }
    session
        .client
        .request(Method::PUT, &path, Some(&full))
        .await?;
    Ok(previous)
}

/// The plan "prepare the server" would follow: the largest context the model supports and this
/// machine's GPU holds (`harness_core::prepare`). `None` for a remote instance (its hardware is
/// not this machine's) or a definition without a single local model.
async fn prepare_plan(
    session: &Session,
    def: &Value,
    tool: LaunchTool,
    managed: bool,
) -> Option<PreparePlan> {
    if session.via == crate::session::Via::Remote {
        return None;
    }
    let path = def["base"]["path"].as_str().filter(|p| !p.is_empty())?;
    // Size of all shards, from the library.
    let size = session
        .client
        .get("/models")
        .await
        .ok()
        .and_then(|m| {
            m["entries"]
                .as_array()?
                .iter()
                .find(|e| e["model"]["id"].as_str() == Some(path))?["model"]["size_bytes"]
                .as_u64()
        })
        .unwrap_or(0);
    let base = harness_core::vram::FitInput {
        model_path: Some(path.to_string()),
        model_size_bytes: size,
        mmproj_path: def["base"]["mmproj_path"]
            .as_str()
            .filter(|p| !p.is_empty())
            .map(str::to_string),
        ..Default::default()
    };
    let need = prepare::ToolNeed::for_tool(tool.id(), tool.min_ctx());
    let available = available_memory(session, def, managed).await;
    tokio::task::spawn_blocking(move || {
        prepare::plan_for_input_with_available_memory(
            base,
            &harness_core::hardware::detect_hardware(),
            need,
            available,
        )
    })
    .await
    .ok()
}

async fn available_memory(
    session: &Session,
    def: &Value,
    managed: bool,
) -> Option<prepare::AvailableMemory> {
    if session.via != crate::session::Via::Owner {
        return None;
    }
    let id = str_of(&def["id"]);
    let report = session
        .client
        .get(&format!(
            "/hardware/devices?refresh=true&definition_id={}",
            seg(&id),
        ))
        .await
        .ok()?;
    let devices: harness_core::devices::DeviceReport =
        serde_json::from_value(report.clone()).ok()?;
    if devices.error.is_some() {
        return None;
    }
    // Older APIs cannot confirm the definition's runtime and adapter.
    let index = report["main_device"].as_u64()? as u32;
    let adapter = report["main_adapter"].as_u64()? as u32;
    let gateway = session.client.get("/gateway").await.ok()?;
    let headroom = gateway["settings"]["memory_headroom_mib"].as_u64()?;
    let slots: Vec<harness_core::gateway::scheduler::ModelSlot> =
        serde_json::from_value(gateway["models"].clone()).ok()?;
    let slots = if managed { slots } else { Vec::new() };
    let servers = session.client.get("/servers").await.ok()?;
    let running: Vec<&Value> = servers
        .as_array()?
        .iter()
        .filter(|s| s["status"] == "Running")
        .collect();
    let stopping: Vec<String> = running
        .iter()
        .filter(|s| s["definition_id"] == id)
        .map(|s| str_of(&s["id"]))
        .collect();
    let mut footprints = Vec::new();
    let mut retained_bytes = None;
    for server in running {
        let server = str_of(&server["id"]);
        if !stopping.contains(&server)
            && !slots.iter().any(|s| {
                prepare::evictable_model(s, &id) && s.server_id.as_deref() == Some(&server)
            })
        {
            continue;
        }
        if let Ok(metrics) = session
            .client
            .get(&format!("/servers/{}/metrics", seg(&server)))
            .await
        {
            if let Some(bytes) =
                retained_vram(&metrics["snapshot"], adapter, &stopping, &slots, &id)
            {
                retained_bytes = Some(retained_bytes.unwrap_or(0u64).max(bytes));
            }
            footprints.push(prepare::ServerMemory {
                bytes: server_vram(&metrics["snapshot"], adapter, &server),
                server_id: server,
                adapter,
            });
        }
    }
    let mut available = prepare::available_memory(
        &devices,
        (index, adapter),
        headroom.saturating_mul(1024 * 1024),
        &id,
        &slots,
        &footprints,
        &stopping,
    )?;
    if let Some(retained) = retained_bytes {
        available.available_vram_bytes = available
            .available_vram_bytes
            .min(available.total_vram_bytes.saturating_sub(retained));
    }
    Some(available)
}

fn retained_vram(
    snapshot: &Value,
    adapter: u32,
    stopping: &[String],
    slots: &[harness_core::gateway::scheduler::ModelSlot],
    target: &str,
) -> Option<u64> {
    let breakdown = match snapshot["gpus"].as_array().filter(|g| !g.is_empty()) {
        Some(gpus) => &gpus.iter().find(|g| g["index"] == adapter)?["breakdown"],
        None if adapter == 0 => &snapshot["vram_breakdown"],
        None => return None,
    };
    let retained = breakdown["other_bytes"]
        .as_u64()?
        .saturating_add(breakdown["harness_bytes"].as_u64()?);
    Some(
        breakdown["servers"]
            .as_array()?
            .iter()
            .filter(|server| {
                let id = str_of(&server["id"]);
                !stopping.contains(&id)
                    && !slots.iter().any(|slot| {
                        prepare::evictable_model(slot, target)
                            && slot.server_id.as_deref() == Some(&id)
                    })
            })
            .fold(retained, |sum, server| {
                sum.saturating_add(server["bytes"].as_u64().unwrap_or(0))
            }),
    )
}

fn server_vram(snapshot: &Value, adapter: u32, server: &str) -> u64 {
    let gpus = snapshot["gpus"].as_array();
    let breakdown = match gpus.filter(|g| !g.is_empty()) {
        Some(g) => g
            .iter()
            .find(|g| g["index"] == adapter)
            .map(|g| &g["breakdown"]),
        None if adapter == 0 => Some(&snapshot["vram_breakdown"]),
        None => None,
    };
    breakdown
        .and_then(|b| b["servers"].as_array())
        .and_then(|s| s.iter().find(|s| s["id"] == server))
        .and_then(|s| s["bytes"].as_u64())
        .unwrap_or(0)
}

fn prepared_context(ctx: Option<u32>, values: &Value) -> Option<u32> {
    values["context_size"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or(ctx)
}

fn should_restore_load(error: &CliError) -> bool {
    error.exit == Exit::OutOfMemory
        || error.code.contains("insufficient_memory")
        || error.code.contains("out_of_memory")
        || error.code == "oom"
        || crate::messages::mentions_out_of_memory(&error.message)
}

fn should_recover(prepared: bool, stopped: &[String], error: &CliError) -> bool {
    prepared && (!stopped.is_empty() || should_restore_load(error))
}

/// Preserve unset values too: the definition may have changed since this server started.
fn server_restore_overrides(
    def: &Value,
    config: &harness_core::process::LaunchConfig,
) -> CliResult<harness_core::params::ParamValues> {
    let def = serde_json::from_value::<harness_core::domain::ModelDefinition>(def.clone())
        .map_err(|e| CliError::general("cli.server_failed", e.to_string()))?;
    Ok(harness_core::domain::launch::snapshot_overrides(
        Some(&def),
        config,
    ))
}

async fn restore_load(session: &Session, id: &str, load: &Value, out: &Out) -> CliResult<()> {
    let path = format!("/definitions/{}", seg(id));
    let mut full = session.client.get(&path).await?;
    // A definition without load settings had the defaults: restore an empty object.
    full["load"] = if load.is_object() {
        load.clone()
    } else {
        json!({})
    };
    session
        .client
        .request(Method::PUT, &path, Some(&full))
        .await?;
    out.warn("Loading failed; the definition's previous load settings were restored.");
    Ok(())
}

/// Definition `load` values of one option: context, flash attention, KV cache, automatic
/// concurrent requests and `-ngl`: the partial offload's layer count, otherwise 999 (all) so an
/// earlier partial value does not stay behind (the GUI's `plan_definition_prepare` does the same).
fn option_values(o: &PrepareOption) -> Value {
    json!({
        "context_size": o.context_size,
        "flash_attn": true,
        "cache_type_k": o.cache_type_k,
        "cache_type_v": o.cache_type_v,
        "parallel_slots": null,
        "gpu_layers": o.gpu_layers.unwrap_or(999),
    })
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// `q8_0` when K and V match, else `K q8_0 / V q4_0`.
fn kv_text(o: &PrepareOption) -> String {
    if o.cache_type_k == o.cache_type_v {
        o.cache_type_k.clone()
    } else {
        format!("K {} / V {}", o.cache_type_k, o.cache_type_v)
    }
}

fn option_text(o: &PrepareOption, plan: &PreparePlan) -> String {
    let base = format!(
        "{} context, {} KV, {}/{} {}",
        o.context_size,
        kv_text(o),
        gib(if plan.cpu_only {
            o.ram_bytes
        } else {
            o.vram_bytes
        }),
        gib(if plan.cpu_only {
            plan.budget_ram_bytes
        } else {
            plan.budget_vram_bytes
        }),
        if plan.cpu_only { "RAM" } else { "VRAM" },
    );
    match (o.kind, o.gpu_layers) {
        (OptionKind::Offload, Some(n)) => format!(
            "{}, {} layers on the GPU, {} RAM (slower)",
            base,
            n,
            gib(o.ram_bytes)
        ),
        _ => base,
    }
}

/// Why the choice is what it is, and what else there is (printed before anything is applied).
/// "X of VRAM is available now ..., Y kept free as headroom" for a `Limit::Available` plan.
fn available_text(plan: &PreparePlan) -> String {
    let a = plan.available_memory.unwrap_or(prepare::AvailableMemory {
        total_vram_bytes: 0,
        available_vram_bytes: 0,
        headroom_bytes: 0,
    });
    format!(
        "{} of VRAM is available now (including evictable idle models and the model itself, which is reloaded), {} of it kept free as headroom",
        gib(a.available_vram_bytes),
        gib(a.headroom_bytes)
    )
}

fn plan_lines(tool: LaunchTool, plan: &PreparePlan) -> Vec<String> {
    use prepare::Limit;
    let mut lines = Vec::new();
    if !plan.cpu_only && plan.available_memory.is_none() {
        lines.push(
            "Current free VRAM is unavailable; this plan uses total device capacity only.".into(),
        );
    }
    match (&plan.choice, plan.verdict) {
        (Some(c), Verdict::Recommended) => lines.push(format!("Choice: {}", option_text(c, plan))),
        (Some(c), _) => lines.push(format!(
            "Choice: {} ({} recommends {} of context; that does not fit)",
            option_text(c, plan),
            tool.name(),
            plan.need.recommended
        )),
        (None, _) => lines.push(format!(
            "{} needs at least {} of context; this model does not fit the GPU fully at that size",
            tool.name(),
            plan.need.min
        )),
    }
    if plan.available_memory.is_some() {
        lines.push(format!("  Limit: {}", available_text(plan)));
    } else if plan.choice.is_some() {
        lines.push(match plan.limit {
            Limit::Ceiling => format!(
                "  Limit: more than {} is pointless for {}",
                plan.need.ceiling,
                tool.name()
            ),
            Limit::Model => format!("  Limit: the model supports at most {}", plan.model.max),
            Limit::Memory => {
                "  Limit: a larger context does not fit in device memory (90% of it is used)".into()
            }
            Limit::Available => format!(
                "  Limit: {} (other programs use the rest)",
                available_text(plan)
            ),
        });
    } else if plan.limit == Limit::Available {
        lines.push(format!("  Limit: {}", available_text(plan)));
    } else if plan.limit == Limit::Model {
        lines.push(format!(
            "  Limit: the model supports at most {}",
            plan.model.max
        ));
    }
    if plan.model.extended_by_rope {
        lines.push(format!(
            "  The file declares RoPE scaling ({}): trained context {}, {} with the scaling",
            plan.model.rope.as_deref().unwrap_or(""),
            plan.model.trained,
            plan.model.max
        ));
    }
    for a in &plan.alternatives {
        let what = match a.kind {
            OptionKind::Q8Kv => "Alternative (better KV quality)",
            OptionKind::F16Kv => "Alternative (best KV quality)",
            OptionKind::LargerCtx => "Alternative (larger context, 8-bit KV cache)",
            OptionKind::Offload => "Alternative (partial CPU offload)",
            OptionKind::Chosen => "Alternative",
        };
        lines.push(format!("{}: {}", what, option_text(a, plan)));
    }
    if plan.suggest_smaller_model {
        lines.push(
            "A smaller model would fit this tool fully on the GPU (see `harness models`)".into(),
        );
    }
    lines
}

fn verdict_text(tool: LaunchTool, v: &CtxVerdict) -> String {
    match v {
        CtxVerdict::Ok => "ok".into(),
        CtxVerdict::TooSmall { have, min, .. } => format!(
            "{} needs at least {} tokens of context; this model has {}",
            tool.name(),
            min,
            have
        ),
        CtxVerdict::Unknown { min, .. } => format!(
            "{} needs at least {} tokens of context; the model is not loaded and its definition sets no context (the server default is usually smaller)",
            tool.name(),
            min
        ),
    }
}

pub async fn run(session: &Session, args: &LaunchArgs, out: &Out) -> CliResult<()> {
    let tool: LaunchTool = args.tool.into();
    let client = &session.client;
    let def = pick_definition(session, args.model.as_deref(), out).await?;
    let def_id = str_of(&def["id"]);
    let alias = str_of(&def["alias"]);

    // Endpoint.
    let managed = if args.direct {
        None
    } else {
        crate::chat::managed_endpoint(session).await
    };
    let mut running: Vec<Value> = servers_of(client, &def_id)
        .await?
        .into_iter()
        .filter(|s| s["status"] == "Running")
        .collect();
    let key_needed = running
        .first()
        .is_some_and(|v| v["active"]["api_key_set"] == true);
    if managed.is_none() && key_needed && args.api_key.is_none() {
        return Err(CliError::usage(
            "cli.invalid_argument",
            "This server requires an API key: pass --api-key or set HARNESS_SERVER_API_KEY",
        ));
    }

    // Context.
    let mut ctx = match running.first() {
        Some(v) => running_ctx(session, v, args.api_key.as_deref()).await,
        None => load_u32(&def, "context_size"),
    };
    let verdict = launch::check_context(tool, ctx);
    let needs_prepare = !matches!(verdict, CtxVerdict::Ok) && !args.no_prepare;
    let prep = if needs_prepare {
        prepare_plan(session, &def, tool, managed.is_some()).await
    } else {
        None
    };
    // What would be written: the plan's full-GPU choice; without a plan (remote instance,
    // unreadable model) the fixed recommendation.
    let prepare_values = match &prep {
        Some(p) => p
            .choice
            .as_ref()
            .or_else(|| {
                p.alternatives
                    .iter()
                    .find(|a| a.kind == OptionKind::Offload)
            })
            .map(option_values)
            .unwrap_or(Value::Null),
        None => launch::prepare_values(tool, ctx),
    };
    let nothing_serves = running.is_empty() || needs_prepare;
    if needs_prepare && prepare_values.is_null() {
        // Say why before refusing: the plan's limit (e.g. the VRAM free now).
        out.warn(verdict_text(tool, &verdict));
        for l in prep
            .as_ref()
            .map(|p| plan_lines(tool, p))
            .unwrap_or_default()
        {
            out.info(l);
        }
        return Err(CliError::new(Exit::OutOfMemory, "cli.launch_context",
            "No preparation option fits the available memory. The definition was not changed; choose a smaller model or free GPU memory."));
    }
    let mut previous_load = None;
    let mut stopped_servers = Vec::new();
    let mut previous_server_settings = std::collections::HashMap::new();

    if args.dry_run {
        let ctx = if needs_prepare {
            prepared_context(ctx, &prepare_values)
        } else {
            ctx
        };
        let endpoint = match &managed {
            Some(m) => Endpoint {
                host_url: host_of(&m.base_url),
                token: m.token.clone(),
                managed: true,
            },
            None => Endpoint {
                host_url: running
                    .first()
                    .and_then(|v| v["endpoint"].as_str())
                    .map(|e| host_of(&session.reachable_endpoint(e)))
                    .unwrap_or_else(|| "http://<not-loaded>".into()),
                token: args.api_key.clone(),
                managed: false,
            },
        };
        let temp = std::env::temp_dir().join("harness-launch-<random>");
        let plan = launch::plan(&LaunchInput {
            tool,
            hide_external_skills: !args.keep_skills,
            host_url: endpoint.host_url.clone(),
            alias: alias.clone(),
            token: endpoint.token.clone(),
            context: ctx,
            managed: endpoint.managed,
            extra_args: args.args.clone(),
            temp_dir: temp,
        });
        let program = tool.find_executable();
        let value = json!({
            "tool": tool.id(),
            "model": alias,
            "definition_id": def_id,
            "endpoint": {
                "kind": if endpoint.managed { "managed" } else { "direct" },
                "base_url": format!("{}/v1", endpoint.host_url),
                "token": endpoint.token.as_ref().map(|_| "***"),
            },
            "context": {
                "per_request": ctx,
                "min": tool.min_ctx(),
                "recommended": tool.recommended_ctx(),
                "check": launch::check_context(tool, ctx),
            },
            "prepare": if needs_prepare { json!({"definition_load": prepare_values, "plan": prep}) } else { Value::Null },
            "load": if nothing_serves {
                json!(if endpoint.managed { "gateway (JIT, warm-up request)" } else { "new server (POST /load)" })
            } else { Value::Null },
            "program_path": program.as_ref().map(|p| p.to_string_lossy().to_string()),
            "plan": plan.redacted(),
        });
        out.data(&value, || {
            let mut s = format!(
                "{} -> {} via {} endpoint {}/v1\n",
                tool.name(),
                alias,
                if endpoint.managed {
                    "managed"
                } else {
                    "direct"
                },
                endpoint.host_url
            );
            s.push_str(&format!(
                "context: {} (min {}, recommended {})\n",
                ctx.map(|c| c.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                tool.min_ctx(),
                tool.recommended_ctx()
            ));
            if needs_prepare {
                if let Some(p) = &prep {
                    for l in plan_lines(tool, p) {
                        s.push_str(&l);
                        s.push('\n');
                    }
                }
                if !prepare_values.is_null() {
                    s.push_str(&format!(
                        "would prepare the definition: {}\n",
                        prepare_values
                    ));
                }
            }
            s.push_str(&format!(
                "program: {}\n",
                program
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{} (not found on PATH)", tool.program()))
            ));
            s.push_str(&serde_json::to_string_pretty(&plan.redacted()).unwrap_or_default());
            s
        });
        return Ok(());
    }

    let program = tool.find_executable().ok_or_else(|| {
        CliError::new(
            Exit::NotFound,
            "cli.launch_not_installed",
            format!(
                "{} is not installed (`{}` is not on PATH)",
                tool.name(),
                tool.program()
            ),
        )
        .with("tool", tool.id())
    })?;

    // Prepare the server when the context is too small (or unknown).
    if needs_prepare {
        out.warn(verdict_text(tool, &verdict));
        if let Some(p) = &prep {
            for l in plan_lines(tool, p) {
                out.info(l);
            }
        }
        // `--prepare` consents to the full-GPU choice only. Going past the GPU (partial
        // offload) is never implied by it.
        let (prepare_values, question, consented) = match &prep {
            Some(p) => match &p.choice {
                Some(c) => (
                    option_values(c),
                    format!(
                        "Prepare the server for {}? The definition \"{}\" gets {}, flash attention and automatic concurrent requests, and is reloaded.",
                        tool.name(),
                        alias,
                        option_text(c, p)
                    ),
                    args.prepare,
                ),
                None => match p.alternatives.iter().find(|a| a.kind == OptionKind::Offload) {
                    Some(o) => (
                        option_values(o),
                        format!(
                            "{} does not fit the GPU fully. Load \"{}\" with {} instead?",
                            tool.name(),
                            alias,
                            option_text(o, p)
                        ),
                        false,
                    ),
                    None => {
                        return Err(CliError::new(
                            Exit::OutOfMemory,
                            "cli.launch_context",
                            format!(
                                "{}. Pick a smaller model, or pass --no-prepare to start anyway",
                                verdict_text(tool, &verdict)
                            ),
                        ))
                    }
                },
            },
            None => (
                prepare_values.clone(),
                format!(
                    "Prepare the server for {}? The definition \"{}\" gets context {}, flash attention, a q8_0 KV cache and automatic concurrent requests, and is reloaded.",
                    tool.name(),
                    alias,
                    prepare_values["context_size"]
                ),
                args.prepare,
            ),
        };
        if !consented {
            out.confirm(&question).map_err(|e| {
                if e.code == "cli.confirmation_required" {
                    CliError::usage(
                        "cli.launch_context",
                        format!(
                            "{}. Pass --prepare to prepare the server (--yes to accept a partial CPU offload) or --no-prepare to start anyway",
                            verdict_text(tool, &verdict)
                        ),
                    )
                } else {
                    e
                }
            })?;
        }
        for s in &running {
            let id = str_of(&s["id"]);
            let effective = client
                .get(&format!("/servers/{}/effective", seg(&id)))
                .await?;
            let config: harness_core::process::LaunchConfig =
                serde_json::from_value(effective["config"].clone()).map_err(|e| {
                    CliError::general(
                        "cli.server_failed",
                        format!("Could not preserve {} load settings: {}", id, e),
                    )
                })?;
            previous_server_settings.insert(id.clone(), server_restore_overrides(&def, &config)?);
        }
        previous_load = Some(prepare_definition(session, &def, &prepare_values).await?);
        ctx = prepared_context(ctx, &prepare_values);
        out.info(format!(
            "Definition \"{}\" updated: {}",
            alias, prepare_values
        ));
        for s in &running {
            let id = str_of(&s["id"]);
            out.info(format!(
                "Stopping {} (reloads with the new settings)...",
                id
            ));
            client
                .post(&format!("/servers/{}/stop", seg(&id)), &json!({}))
                .await?;
            stopped_servers.push(id);
        }
        running.clear();
    } else if !matches!(verdict, CtxVerdict::Ok) {
        out.warn(format!(
            "{} (starting anyway: --no-prepare)",
            verdict_text(tool, &verdict)
        ));
    }

    // Something must serve the model.
    let loaded: CliResult<Endpoint> = async {
        Ok(match &managed {
            Some(m) => {
                let e = Endpoint {
                    host_url: host_of(&m.base_url),
                    token: m.token.clone(),
                    managed: true,
                };
                if running.is_empty() {
                    warm_up(&e, &alias, out).await?;
                }
                e
            }
            None => {
                if running.is_empty() {
                    out.info(format!("Loading {}...", alias));
                    let reuse = idle_server_of(client, &def_id).await?;
                    let loaded = client
                        .post(
                            "/load",
                            &json!({"definition_id": def_id, "server_id": reuse}),
                        )
                        .await?;
                    let id = str_of(&loaded["server_id"]);
                    let view =
                        wait_running(client, &id, Duration::from_secs(args.timeout), out).await?;
                    running.push(view);
                }
                let view = &running[0];
                let endpoint = view["endpoint"].as_str().ok_or_else(|| {
                    CliError::general("cli.server_failed", "The server reports no endpoint")
                })?;
                Endpoint {
                    host_url: host_of(&session.reachable_endpoint(endpoint)),
                    token: args
                        .api_key
                        .clone()
                        .filter(|_| view["active"]["api_key_set"] == true),
                    managed: false,
                }
            }
        })
    }
    .await;
    let endpoint = match loaded {
        Ok(endpoint) => endpoint,
        Err(error) => {
            if let Some(load) = previous_load {
                if let Err(restore) = restore_load(session, &def_id, &load, out).await {
                    out.warn(format!(
                        "Could not restore the previous load settings: {}",
                        restore.message
                    ));
                } else if should_recover(true, &stopped_servers, &error) {
                    for id in &stopped_servers {
                        let restarted: CliResult<()> = async {
                                client
                                    .post(
                                        &format!("/servers/{}/start", seg(id)),
                                        &json!({"definition_id": def_id, "overrides": previous_server_settings.get(id)}),
                                    )
                                    .await?;
                                wait_running(client, id, Duration::from_secs(args.timeout), out)
                                    .await?;
                                Ok(())
                            }
                            .await;
                        match restarted {
                                Ok(()) => out.warn(format!("The definition's previous load settings were restored and {} was restarted.", id)),
                                Err(restart) => out.warn(format!("Previous load settings were restored, but restarting {} failed: {}", id, restart.message)),
                            }
                    }
                }
            }
            return Err(error);
        }
    };
    // The context the model now has (after a prepare / load).
    if let Some(v) = servers_of(client, &def_id)
        .await?
        .iter()
        .find(|s| s["status"] == "Running")
    {
        if let Some(c) = running_ctx(session, v, endpoint.token.as_deref()).await {
            ctx = Some(c);
        }
    }

    let temp_dir = launch::new_temp_dir().map_err(|e| CliError::general("cli.io", e))?;
    let plan = launch::plan(&LaunchInput {
        tool,
        hide_external_skills: !args.keep_skills,
        host_url: endpoint.host_url.clone(),
        alias: alias.clone(),
        token: endpoint.token.clone(),
        context: ctx,
        managed: endpoint.managed,
        extra_args: args.args.clone(),
        temp_dir: temp_dir.clone(),
    });
    let result = spawn_and_wait(&plan, &program, out).await;
    let _ = std::fs::remove_dir_all(&temp_dir);
    let code = result?;
    CHILD_EXIT.store(code, Ordering::SeqCst);
    Ok(())
}

async fn spawn_and_wait(
    plan: &launch::LaunchPlan,
    program: &std::path::Path,
    out: &Out,
) -> CliResult<i32> {
    plan.write_files()
        .map_err(|e| CliError::general("cli.io", e))?;
    out.info(format!(
        "Starting {} with {} ({} endpoint {})",
        plan.tool.id(),
        plan.model,
        if plan.managed { "managed" } else { "direct" },
        plan.base_url
    ));
    let mut cmd = plan.command();
    let mut child = cmd.spawn().map_err(|e| {
        CliError::general(
            "cli.launch_failed",
            format!("Could not start {}: {}", program.display(), e),
        )
    })?;
    // The tool owns the terminal; Ctrl+C reaches it directly and must not end `harness` (which
    // cleans up the temp files once the tool exits).
    let wait = tokio::task::spawn_blocking(move || child.wait());
    tokio::pin!(wait);
    let status = loop {
        tokio::select! {
            r = &mut wait => break r,
            _ = tokio::signal::ctrl_c() => continue,
        }
    };
    let status = status
        .map_err(|e| CliError::general("cli.io", e.to_string()))?
        .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_gpu_option_clears_an_earlier_partial_offload() {
        let mut o = PrepareOption {
            context_size: 65536,
            kv_level: harness_core::kv_policy::KvLevel::Balanced,
            cache_type_k: "q8_0".into(),
            cache_type_v: "q8_0".into(),
            gpu_layers: None,
            vram_bytes: 0,
            ram_bytes: 0,
            percent: 0,
            kind: OptionKind::Chosen,
        };
        assert_eq!(option_values(&o)["gpu_layers"], 999);
        o.gpu_layers = Some(13);
        o.kind = OptionKind::Offload;
        assert_eq!(option_values(&o)["gpu_layers"], 13);
    }

    #[test]
    fn memory_load_failures_restore_previous_settings() {
        for code in ["insufficient_memory", "servers.out_of_memory", "oom"] {
            assert!(should_restore_load(&CliError::general(code, "load failed")));
        }
        assert!(should_restore_load(&CliError::new(
            Exit::OutOfMemory,
            "cli.launch_load_failed",
            "failed"
        )));
        assert!(!should_restore_load(&CliError::general(
            "cli.timeout",
            "timed out"
        )));
        assert!(!should_restore_load(&CliError::general(
            "servers.port_in_use",
            "port in use"
        )));
    }

    #[test]
    fn dry_run_tool_context_matches_preparation() {
        let values = json!({"context_size": 65536});
        assert_eq!(prepared_context(Some(32768), &values), Some(65536));
        assert_eq!(prepared_context(Some(49152), &Value::Null), Some(49152));
        assert_eq!(prepared_context(None, &Value::Null), None);
        for tool in [LaunchTool::OpenCode, LaunchTool::Codex] {
            let plan = launch::plan(&LaunchInput {
                tool,
                hide_external_skills: true,
                host_url: "http://example.invalid".into(),
                alias: "test".into(),
                token: None,
                context: prepared_context(None, &values),
                managed: true,
                extra_args: vec![],
                temp_dir: std::path::PathBuf::from("unused"),
            });
            if tool == LaunchTool::OpenCode {
                let content = plan
                    .env
                    .iter()
                    .find(|(key, _)| key == "OPENCODE_CONFIG_CONTENT")
                    .unwrap();
                let config: Value = serde_json::from_str(&content.1).unwrap();
                assert_eq!(
                    config["provider"]["harness"]["models"]["test"]["limit"]["context"],
                    65536
                );
            } else {
                assert!(plan.args.contains(&"model_context_window=65536".into()));
            }
        }
    }

    #[test]
    fn preparation_failure_restores_stopped_servers() {
        let error = CliError::general("servers.failed", "load failed");
        assert!(should_recover(true, &["default".into()], &error));
        assert!(!should_recover(false, &["default".into()], &error));
        assert!(!should_recover(true, &[], &error));
    }

    #[test]
    fn preparation_rollback_clears_settings_added_after_the_server_started() {
        use harness_core::domain::{ModelDefinition, ModelRef};
        use harness_core::params::ParamValue;
        let mut def = ModelDefinition::new(
            "test",
            ModelRef::Local {
                path: "m.gguf".into(),
            },
        );
        def.extra_args = Some("-c 1048576 -ctk f16 -ctv f16".into());
        def.load.insert("flash_attn".into(), ParamValue::Bool(true));
        def.load
            .insert("cache_type_k".into(), ParamValue::Text("q8_0".into()));
        let mut old = harness_core::process::LaunchConfig {
            context_size: 4096,
            gpu_layers: 999,
            ..Default::default()
        };
        let overrides =
            server_restore_overrides(&serde_json::to_value(&def).unwrap(), &old).unwrap();
        let mut changed = old.clone();
        changed.custom_args = def.extra_args;
        changed.flash_attn = true;
        changed.cache_type_k = Some("q8_0".into());
        harness_core::domain::launch::apply_layer(&mut changed, &overrides).unwrap();
        assert_eq!(changed.custom_args, None);
        assert!(!changed.flash_attn);
        assert_eq!(changed.cache_type_k, None);
        old.custom_args = Some("--no-warmup".into());
        let overrides = server_restore_overrides(
            &serde_json::to_value(ModelDefinition::new(
                "test",
                ModelRef::Local {
                    path: "m.gguf".into(),
                },
            ))
            .unwrap(),
            &old,
        )
        .unwrap();
        assert_eq!(
            overrides.get("custom_args"),
            Some(&ParamValue::Text("--no-warmup".into()))
        );
    }

    #[test]
    fn available_limit_is_printed_even_when_not_binding() {
        let mut plan = prepare::plan_with_available_memory(
            &harness_core::vram::FitInput::default(),
            None,
            &harness_core::vram::MemoryBudget::default(),
            &[],
            prepare::ToolNeed::for_tool("codex", 0),
            None,
        );
        plan.available_memory = Some(prepare::AvailableMemory {
            total_vram_bytes: 4096 << 20,
            available_vram_bytes: 3295 << 20,
            headroom_bytes: 512 << 20,
        });
        plan.limit = prepare::Limit::Model;
        assert!(plan_lines(LaunchTool::Codex, &plan)
            .iter()
            .any(|line| line == &format!("  Limit: {}", available_text(&plan))));
    }

    #[test]
    fn measured_vram_is_specific_to_server_and_adapter() {
        let snapshot = json!({"gpus": [
            {"index": 0, "breakdown": {"servers": [{"id": "victim", "bytes": 100}]}},
            {"index": 1, "breakdown": {"servers": [{"id": "victim", "bytes": 700}, {"id": "other", "bytes": 900}]}}
        ]});
        assert_eq!(server_vram(&snapshot, 1, "victim"), 700);
        assert_eq!(server_vram(&snapshot, 2, "victim"), 0);
        assert_eq!(server_vram(&Value::Null, 0, "victim"), 0);
        let single =
            json!({"gpus": [], "vram_breakdown": {"servers": [{"id": "victim", "bytes": 500}]}});
        assert_eq!(server_vram(&single, 0, "victim"), 500);
        assert_eq!(server_vram(&single, 1, "victim"), 0);
    }

    #[test]
    fn retained_vram_excludes_target_but_keeps_other_consumers() {
        let snapshot = json!({"gpus": [{"index": 1, "breakdown": {
            "other_bytes": 40, "harness_bytes": 60,
            "servers": [{"id": "target", "bytes": 3800}, {"id": "busy", "bytes": 200}]
        }}]});
        assert_eq!(
            retained_vram(&snapshot, 1, &["target".into()], &[], "def"),
            Some(300)
        );
        assert_eq!(retained_vram(&snapshot, 0, &[], &[], "def"), None);
        assert_eq!(retained_vram(&Value::Null, 1, &[], &[], "def"), None);
    }
}
