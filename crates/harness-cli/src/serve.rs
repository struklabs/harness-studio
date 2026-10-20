//! `harness serve`: the headless owner of the data directory.
//!
//! Same startup as the GUI minus the window: take `instance.lock`, load the download queue,
//! open `state.db`, run the telemetry sampler, start the Control API (always on here; host
//! and port from the settings unless overridden), relaunch auto-start profiles / the last
//! session, then wait for Ctrl+C, SIGTERM, a console close or `POST /shutdown`. A clean
//! shutdown stops every llama-server, stops the API and removes `instance.json`.

use std::path::Path;
use std::sync::Arc;

use harness_core::config::AppSettings;
use serde_json::json;

use crate::cli::ServeArgs;
use crate::error::CliResult;
use crate::output::Out;
use crate::session::{Instance, InstanceOptions};

pub async fn run(args: &ServeArgs, data_dir: &Path, out: &Out) -> CliResult<()> {
    let mut settings = tokio::task::spawn_blocking(|| AppSettings::load().control_api)
        .await
        .unwrap_or_default();
    settings.enabled = true;
    if let Some(host) = &args.listen {
        settings.host = host.clone();
    }
    if let Some(port) = args.port {
        settings.port = port;
    }
    let mut browser: harness_api::browser::BrowserConfig = match &args.serve_config {
        Some(path) => serde_json::from_slice(
            &std::fs::read(path)
                .map_err(|e| crate::error::CliError::general("cli.io", e.to_string()))?,
        )
        .map_err(|e| crate::error::CliError::general("cli.io", e.to_string()))?,
        None => Default::default(),
    };
    if let Some(dir) = &args.ui_dir {
        browser.ui_dir = dir.clone();
    }
    if browser.ui_dir.as_os_str().is_empty() {
        let packaged = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.join("web")));
        browser.ui_dir = packaged
            .filter(|p| p.join("index.html").is_file())
            .unwrap_or_else(|| "build".into());
    }
    let browser = if browser.ui_dir.join("index.html").is_file() {
        Some(browser)
    } else {
        if args.ui_dir.is_some() || args.serve_config.is_some() {
            return Err(crate::error::CliError::core("browser.ui_missing"));
        }
        out.info("warning: browser UI build missing; run npm run build or pass --ui-dir. Only the Control API is available.");
        None
    };
    if !harness_core::network::is_loopback_host(&settings.host)
        && !browser.as_ref().is_some_and(|b| b.tls_cert.is_some())
    {
        out.info("WARNING: plain HTTP on a non-loopback address exposes API traffic. Browser password login requires HTTPS. Configure tls_cert and tls_key.");
    }
    let browser_enabled = browser.is_some();
    let browser_origin = browser.as_ref().and_then(|b| b.public_origin.clone());
    harness_api::enable_mcp_follow();
    let stop = Arc::new(tokio::sync::Notify::new());
    let instance = Instance::start(
        data_dir,
        InstanceOptions {
            browser,
            settings,
            ephemeral_port: false,
            strict_port: args.port.is_some(),
            shutdown: Some(stop.clone()),
        },
    )
    .await?;

    if let Some(code) = &instance.setup_code {
        // This is a one-time pairing secret: terminal stderr only, never ready JSON/log events.
        eprintln!("Browser Admin Setup Code (Valid for 15 Minutes): {code}");
    }
    if browser_enabled {
        out.info(format!(
            "Browser Management: {}",
            browser_origin.as_deref().unwrap_or_else(|| instance
                .base_url
                .trim_end_matches(harness_core::control_api::BASE_PATH))
        ));
    }

    // Same one-time work as the GUI's startup (both are owners).
    tokio::task::spawn_blocking(|| {
        harness_core::mcp::remove_all_runtime_configs();
        harness_core::models::ModelManager::migrate_secrets();
        if let Err(e) = harness_core::state::shared() {
            eprintln!("warning: state.db unavailable: {}", e);
        }
    })
    .await
    .ok();
    // Request analytics: VRAM/RAM peaks come from the telemetry history.
    harness_core::analytics::set_telemetry(instance.ctx.telemetry.clone());
    // Gateway (managed endpoint): on with `--gateway` or the setting, off with `--no-gateway`.
    let gateway = harness_gateway::GatewayServer::new();
    gateway.set_strict_port(args.gateway_port.is_some());
    let mut gw_settings = tokio::task::spawn_blocking(|| AppSettings::load().gateway)
        .await
        .unwrap_or_default();
    if let Some(port) = args.gateway_port {
        gw_settings.port = port;
    }
    let gateway_on = !args.no_gateway && (args.gateway || gw_settings.enabled);
    let mut gateway_error = None;
    if gateway_on {
        harness_gateway::enable_client_follow();
        let ctx = harness_gateway::GatewayContext {
            registry: instance.ctx.registry.clone(),
            runtime: instance.ctx.runtime.clone(),
            gate: instance.ctx.gate.clone(),
            data_dir: data_dir.to_path_buf(),
            downloads: Some(instance.ctx.downloads.clone()),
        };
        if let Err(e) = gateway.start(ctx, &gw_settings).await {
            out.info(format!("warning: gateway not started: {}", e));
            gateway_error = Some(e);
        }
    }
    // OpenTelemetry export of the gateway's requests (opt-in in the settings).
    let otel_settings = tokio::task::spawn_blocking(|| AppSettings::load().otel)
        .await
        .unwrap_or_default();
    if otel_settings.enabled {
        if let Err(e) = harness_core::otel::apply(&otel_settings).await {
            out.info(format!(
                "warning: OpenTelemetry export not started: {}",
                crate::messages::core_text(&e)
            ));
        }
    }
    let gateway_status = harness_core::gateway::status();
    if let (Some(fb), Some(port)) = (&gateway_status.port_fallback, gateway_status.port) {
        out.info(format!(
            "warning: port {} is in use ({}); the gateway runs on port {}",
            fb.configured,
            fb.holder
                .as_ref()
                .map(|h| h.label())
                .unwrap_or_else(|| "another program".into()),
            port
        ));
    }
    if let Some(w) = &gateway_status.ollama_warning {
        let (key, rest) = w.split_once(": ").unwrap_or((w.as_str(), ""));
        out.info(format!(
            "warning: {}: {}",
            crate::messages::base(key).unwrap_or(key),
            rest
        ));
    }
    let sampler = tokio::spawn(harness_core::orchestrator::run_telemetry_sampler(
        instance.ctx.telemetry.clone(),
        instance.ctx.registry.clone(),
        |_| {},
    ));

    let mut ready = instance.describe();
    ready["event"] = json!("ready");
    ready["version"] = json!(crate::session::VERSION);
    ready["gateway"] = json!({
        "running": gateway_status.running,
        "base_url": gateway_status.endpoint.as_ref().map(|e| e.base_url.clone()),
        "require_token": gateway_status.require_token,
        "error": gateway_error,
        "ollama_port": gateway_status.ollama_port,
        "ollama_warning": gateway_status.ollama_warning,
        "port_fallback": gateway_status.port_fallback,
    });
    if out.json {
        out.json_line(&ready);
    } else {
        println!(
            "Harness headless instance ready\n  Control API: {}\n  token file:  {}\n  data dir:    {}\nPress Ctrl+C to stop.",
            instance.base_url,
            ready["token_path"].as_str().unwrap_or("-"),
            data_dir.display()
        );
    }

    let restore = (!args.no_restore).then(|| {
        tokio::spawn(harness_core::orchestrator::restore_servers(
            instance.ctx.registry.clone(),
            instance.ctx.runtime.clone(),
            instance.ctx.gate.clone(),
        ))
    });

    let reason = wait_for_stop(&stop).await;
    out.info(format!("Shutting down ({})...", reason));
    if let Some(task) = restore {
        task.abort();
    }
    sampler.abort();
    gateway.stop().await;
    // The Lemonade engine's lemond (started by a request for one of its models).
    harness_core::lemonade::shutdown_now();
    // Requests still waiting for their telemetry sample are written now.
    let _ = tokio::task::spawn_blocking(|| {
        harness_core::analytics::flush(std::time::Duration::from_secs(3))
    })
    .await;
    // Spans and metrics still queued are sent now (bounded by the export timeout).
    harness_core::otel::shutdown().await;
    instance.shutdown().await;
    if out.json {
        out.json_line(&json!({"event": "stopped", "reason": reason}));
    } else {
        out.info("Stopped.");
    }
    Ok(())
}

/// Waits for Ctrl+C, SIGTERM/SIGHUP (Unix), a console close/logoff/shutdown (Windows) or the
/// API's `POST /shutdown`.
async fn wait_for_stop(api: &tokio::sync::Notify) -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).ok();
        let mut hup = signal(SignalKind::hangup()).ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "ctrl-c",
            _ = async { match term.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => "sigterm",
            _ = async { match hup.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => "sighup",
            _ = api.notified() => "api",
        }
    }
    #[cfg(windows)]
    {
        use tokio::signal::windows;
        let mut close = windows::ctrl_close().ok();
        let mut shutdown = windows::ctrl_shutdown().ok();
        let mut logoff = windows::ctrl_logoff().ok();
        let mut brk = windows::ctrl_break().ok();
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "ctrl-c",
            _ = async { match brk.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => "ctrl-break",
            _ = async { match close.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => "console-close",
            _ = async { match shutdown.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => "system-shutdown",
            _ = async { match logoff.as_mut() { Some(s) => { s.recv().await; } None => std::future::pending().await } } => "logoff",
            _ = api.notified() => "api",
        }
    }
}
