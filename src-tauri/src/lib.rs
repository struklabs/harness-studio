pub mod app_update;
#[cfg(target_os = "linux")]
mod autostart_linux;
pub mod desktop;
pub mod push;
#[cfg(all(windows, debug_assertions, feature = "qa-cdp"))]
mod qa_cdp;

// Core services live in `harness-core`; re-exported so `crate::<module>` paths keep working.
pub use harness_core::{
    analytics, benchmark, capabilities, config, control_api, devices, diagnostics, doctor,
    downloader, failure, gateway, gguf, gpu, hardware, hf_avatar, importer, instance, integrations,
    lemonade, lora_live, mcp, models, network, onnx_runtime, operation_gate, orchestrator, params,
    paths, perf, prepare, process, recovery, runtime, runtime_update, runtimes, secrets,
    server_logs, servers, state, storage, store, telemetry, tools, vram,
};

pub mod commands;

use std::sync::Arc;
use tauri::{Emitter, Manager};

use app_update::AppUpdateState;
use benchmark::BenchmarkManager;
use commands::*;
use config::AppSettings;
use diagnostics::DiagnosticsManager;
use downloader::DownloadManager;
use models::ModelManager;
use operation_gate::OperationGate;
use orchestrator::restore_servers;
use runtime::RuntimeManager;
use runtime_update::RuntimeAutoUpdater;
use servers::ServerRegistry;
use telemetry::TelemetryPoller;

/// Records Rust panics as local crash events (processed on next start, subject to consent).
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = match info.payload().downcast_ref::<&str>() {
            Some(s) => s.to_string(),
            None => info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "panic".to_string()),
        };
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        DiagnosticsManager::record(
            "crash",
            format!("{} ({})", message, location),
            Some(backtrace),
        );
        default_hook(info);
    }));
}

/// MCP servers started by the app's own client end with the app (stdin closes, then the process
/// tree is killed).
async fn stop_mcp() {
    tools::ToolService::shared()
        .mcp()
        .shutdown(std::time::Duration::from_secs(2))
        .await;
    mcp::remove_all_runtime_configs();
}

/// The part of the exit cleanup that runs before an installer replaces the process (see
/// `app_update`): Lemonade and the MCP servers.
pub(crate) async fn stop_lemonade_and_mcp() {
    let _ = tokio::task::spawn_blocking(harness_core::lemonade::shutdown_now).await;
    stop_mcp().await;
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let registry = Arc::new(ServerRegistry::default());
    let runtime_mgr = Arc::new(RuntimeManager::default());
    let model_mgr = Arc::new(ModelManager);
    // The queue worker may be started from setup or sync commands (outside a tokio context).
    let download_mgr = Arc::new(DownloadManager::with_runtime(
        tauri::async_runtime::handle().inner().clone(),
    ));
    let restore_downloads = download_mgr.clone();
    let bench_mgr = Arc::new(BenchmarkManager::default());
    let operation_gate = Arc::new(OperationGate::default());

    // The GPU (for VRAM telemetry) is looked up after the window is up (see setup).
    let telemetry_poller = Arc::new(tokio::sync::Mutex::new(TelemetryPoller::new(None)));
    let gpu_poller = telemetry_poller.clone();
    let sampler_poller = telemetry_poller.clone();
    // Request analytics: VRAM/RAM peaks come from the telemetry history.
    analytics::set_telemetry(telemetry_poller.clone());
    let sampler_registry = registry.clone();
    let restore_registry = registry.clone();
    let restore_runtime = runtime_mgr.clone();
    let update_runtime = runtime_mgr.clone();
    let update_registry = registry.clone();
    let auto_updater = RuntimeAutoUpdater::default();
    let auto_updater_task = auto_updater.clone();
    let update_gate = operation_gate.clone();
    let restore_gate = operation_gate.clone();
    let control_api = Arc::new(harness_api::ApiServer::new());
    let exit_api = control_api.clone();
    let gateway_server = Arc::new(harness_gateway::GatewayServer::new());
    let exit_gateway = gateway_server.clone();
    let api_services = (
        registry.clone(),
        runtime_mgr.clone(),
        download_mgr.clone(),
        bench_mgr.clone(),
        operation_gate.clone(),
        telemetry_poller.clone(),
    );

    // Test/measurement override: with `HARNESS_DATA_DIR` set (same variable as the CLI's
    // `--data-dir`) the app uses that data dir and skips the single-instance guard, so an
    // isolated copy can run next to the normal one.
    let data_dir_override = std::env::var_os("HARNESS_DATA_DIR")
        .map(std::path::PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty());

    let mut builder = tauri::Builder::default();
    if data_dir_override.is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            desktop::show_main_window(app);
        }));
    }
    builder
        .setup(move |app| {
            // Persistent data lives in the per-user app data dir, independent of the CWD.
            let data_dir = match &data_dir_override {
                Some(dir) => dir.clone(),
                None => app.path().app_data_dir()?,
            };
            paths::init(data_dir.clone());
            // Enumerating GPUs loads the display driver: not on the path to the first window.
            tauri::async_runtime::spawn(async move {
                let adapters = tauri::async_runtime::spawn_blocking(gpu::list_adapters)
                    .await
                    .unwrap_or_default();
                gpu_poller.lock().await.set_adapters(adapters);
            });
            // Background telemetry sampler: history is complete regardless of which tab is
            // open; it rests while no server runs and no view watches. Changes of the watched
            // server are pushed to the window. After `paths::init`: its tray tooltip update
            // reads (and caches) the settings.
            let telemetry_watch = Arc::new(push::TelemetryWatch::default());
            app.manage(telemetry_watch.clone());
            let handle = app.handle().clone();
            let round_handle = app.handle().clone();
            tauri::async_runtime::spawn(orchestrator::run_telemetry_sampler_watched(
                sampler_poller,
                sampler_registry.clone(),
                telemetry_watch.demand(),
                move |count| desktop::update_tray_tooltip(&handle, count),
                move |poller| telemetry_watch.after_round(&round_handle, poller),
            ));
            push::spawn(app.handle(), sampler_registry);

            // Single owner of the data dir (instance.lock / instance.json). The GUI is already
            // single-instance; the lock also keeps a headless `harness serve` out. If another
            // owner holds it, the GUI starts as before but reports it and runs no Control API.
            let version = app.package_info().version.to_string();
            let instance = match instance::InstanceLock::acquire(
                &data_dir,
                instance::InstanceKind::Gui,
                &version,
            ) {
                Ok(lock) => InstanceState::owner(Arc::new(lock)),
                Err(e) => {
                    eprintln!("[Harness Studio] {}", e);
                    InstanceState::not_owner(e)
                }
            };
            control_api.set_instance(instance.lock.clone());
            let owner = instance.lock.is_some();
            app.manage(instance);
            let (registry, runtime, downloads, benchmarks, gate, telemetry) = api_services;
            let api_ctx = harness_api::ApiContext {
                registry,
                runtime,
                downloads,
                benchmarks,
                gate,
                telemetry,
                kind: instance::InstanceKind::Gui,
                version,
                data_dir,
            };
            app.manage(ControlApiState {
                server: control_api.clone(),
                ctx: api_ctx.clone(),
            });
            // Gateway (managed endpoint): optional, in this process, owner only.
            // An isolated dev/test copy must never retarget the real user's client configs.
            if data_dir_override.is_none() {
                harness_gateway::enable_client_follow();
                harness_api::enable_mcp_follow();
            }
            let gw_ctx = harness_gateway::GatewayContext {
                registry: api_ctx.registry.clone(),
                runtime: api_ctx.runtime.clone(),
                gate: api_ctx.gate.clone(),
                data_dir: api_ctx.data_dir.clone(),
                downloads: Some(api_ctx.downloads.clone()),
            };
            app.manage(GatewayState {
                server: gateway_server.clone(),
                ctx: gw_ctx.clone(),
            });
            let gw_settings = AppSettings::load().gateway;
            if owner && gw_settings.enabled {
                let gw = gateway_server.clone();
                let gw_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = gw.start(gw_ctx, &gw_settings).await {
                        eprintln!("[Harness Studio] Gateway başlatılamadı: {}", e);
                    }
                    // The window may have read the gateway status before it was listening.
                    let _ = gw_handle.emit("gateway-changed", ());
                });
            }
            // OpenTelemetry export (opt-in): only the owner runs the gateway it reports on.
            let otel_settings = AppSettings::load().otel;
            if owner && otel_settings.enabled {
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = harness_core::otel::apply(&otel_settings).await {
                        eprintln!(
                            "[Harness Studio] OpenTelemetry dışa aktarma başlatılamadı: {}",
                            e
                        );
                    }
                });
            }
            let api_settings = AppSettings::load().control_api;
            if owner && api_settings.enabled {
                let api = control_api.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = api.start(api_ctx, &api_settings).await {
                        eprintln!("[Harness Studio] Control API başlatılamadı: {}", e);
                    }
                });
            }
            paths::migrate_legacy_data();
            mcp::remove_all_runtime_configs();
            tauri::async_runtime::spawn_blocking(|| {
                let _ = AppSettings::load();
                // Imports presets.json / profiles.json / last_session.json into the v2 files
                // (model definitions + server profiles), keeping `.pre-v2` backups.
                ModelManager::migrate_secrets();
            });
            install_panic_hook();
            restore_downloads.restore();
            app.manage(Arc::new(DiagnosticsManager::default_manager()));
            // Conversations and user state; opened once (migrations run here).
            // Shared with the core (library "last used" is recorded when a server starts).
            let state_db = state::shared();
            if let Err(e) = &state_db {
                eprintln!("[Harness Studio] state.db unavailable: {}", e);
            }
            app.manage(ConversationStore(state_db));

            #[cfg(target_os = "macos")]
            desktop::migrate_launch_agent(app.handle());
            #[cfg(target_os = "linux")]
            autostart_linux::refresh();
            desktop::set_app_menu(app.handle(), &AppSettings::load().language);
            desktop::setup_tray_now_or_later(app.handle());
            // The window starts hidden (tauri.conf.json) to avoid a flash when
            // launched at login with "start minimized".
            let stay_hidden = desktop::launched_minimized()
                && AppSettings::load().startup.start_minimized
                && desktop::tray_available();
            if let Some(window) = app.get_webview_window("main") {
                desktop::apply_window_background(&window);
                desktop::allow_microphone(&window);
                #[cfg(all(windows, debug_assertions, feature = "qa-cdp"))]
                qa_cdp::start(window)?;
            }
            if stay_hidden {
                desktop::started_hidden(app.handle());
            } else {
                desktop::show_main_window(app.handle());
            }
            // After `show`: a window that was never mapped has no monitor yet (GTK).
            desktop::fit_main_window_to_work_area(app.handle());
            tauri::async_runtime::spawn(restore_servers(
                restore_registry,
                restore_runtime,
                restore_gate,
            ));
            let update_handle = app.handle().clone();
            tauri::async_runtime::spawn(auto_updater_task.run_loop(
                update_runtime,
                update_registry,
                update_gate,
                move |settings: &AppSettings| {
                    let _ = update_handle.emit("runtime-updated", settings);
                },
            ));
            Ok(())
        })
        .manage(registry)
        .manage(runtime_mgr)
        .manage(model_mgr)
        .manage(download_mgr)
        .manage(bench_mgr)
        .manage(operation_gate)
        .manage(auto_updater)
        .manage(AppUpdateState::default())
        .manage(commands::PendingExposure::default())
        .manage::<commands::ToolRuns>(tools::RunTable::new())
        .manage(telemetry_poller)
        .manage(Arc::new(perf::PerfSampler::default()))
        .plugin(desktop::autostart_plugin())
        .on_window_event(desktop::on_window_event)
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            desktop::is_tray_available,
            desktop::resolve_window_close,
            desktop::release_window,
            detect_system_hardware,
            get_runtime_devices,
            estimate_fit,
            estimate_image_fit,
            model_picker_memory,
            estimate_launch_fit,
            automatic_launch_choice,
            estimate_definition_load_fit,
            get_backend_status,
            get_app_settings,
            save_app_settings,
            set_audio_cpp_enabled,
            get_network_overview,
            acknowledge_lan_exposure,
            revoke_lan_exposure,
            discard_pending_exposure,
            generate_profile_api_key,
            get_profile_api_key,
            set_hf_token,
            clear_hf_token,
            hf_auth_status,
            hf_oauth_login,
            hf_oauth_cancel,
            hf_whoami,
            hf_logout,
            set_hf_oauth_client_id,
            get_server_api_key,
            check_runtime_update,
            download_and_verify_staging,
            activate_managed_runtime,
            activate_staged_runtime,
            rollback_managed_runtime,
            has_runtime_backup,
            get_runtime_auto_update_status,
            run_runtime_auto_update,
            get_runtime_download_progress,
            validate_custom_binary,
            get_runtime_detection_info,
            get_runtime_capabilities,
            get_param_catalog,
            get_feature_status,
            get_task_support,
            get_effective_launch_values,
            list_runtime_flags,
            auto_configure_system_runtime,
            list_runtimes,
            preview_runtime_release,
            install_runtime_github,
            get_runtime_install_progress,
            get_tts_overview,
            get_audio_process_overview,
            save_processed_audio,
            install_tts_model,
            get_tts_install_progress,
            get_asr_overview,
            install_asr_model,
            get_crispasr_helpers,
            download_crispasr_helper,
            remove_crispasr_helper,
            get_asr_install_progress,
            convert_plan,
            convert_start,
            list_convert_jobs,
            cancel_convert_job,
            remove_convert_job,
            clear_finished_conversions,
            get_convert_env,
            install_convert_env,
            cancel_convert_env_install,
            remove_convert_env,
            list_python_envs,
            install_python_env,
            cancel_python_env_install,
            remove_python_env,
            list_onnx_runtime,
            get_onnx_runtime_support,
            get_onnx_runtime_progress,
            install_onnx_runtime,
            verify_onnx_runtime,
            remove_onnx_runtime,
            get_lemonade_status,
            preview_lemonade_release,
            install_lemonade,
            remove_lemonade_install,
            save_lemonade_settings,
            start_lemonade,
            stop_lemonade,
            list_lemonade_models,
            refresh_lemonade_models,
            get_lemonade_progress,
            pull_lemonade_model,
            delete_lemonade_model,
            install_lemonade_backend,
            get_lemonade_logs,
            add_runtime_local,
            verify_runtime,
            get_runtime_users,
            remove_runtime,
            get_model_runtime_advice,
            get_model_engine,
            start_server,
            stop_server,
            get_recovery_plan,
            list_lora_adapters,
            set_lora_scales,
            retry_server,
            run_doctor,
            get_server_launch_config,
            list_router_models,
            load_router_model,
            unload_router_model,
            tune_server_for_agent,
            plan_server_prepare,
            plan_definition_prepare,
            plan_launch_context,
            prepare_definition_for_agent,
            get_server_status,
            get_server_logs,
            get_server_logs_since,
            clear_server_logs,
            get_active_server,
            list_servers,
            remove_server,
            list_server_profiles,
            get_default_server,
            save_server_profile,
            delete_server_profile,
            start_server_profile,
            list_model_definitions,
            get_model_definition,
            save_model_definition,
            delete_model_definition,
            duplicate_model_definition,
            list_inference_presets,
            get_inference_preset,
            save_inference_preset,
            delete_inference_preset,
            resolve_effective_config,
            get_data_files_status,
            list_chat_templates,
            read_chat_template,
            save_chat_template,
            read_model_chat_template,
            pick_chat_template_file,
            get_definition_refs,
            get_definition_form,
            get_dashboard_definition,
            save_definition_form,
            save_definition_load,
            list_definition_states,
            load_definition,
            export_inference_presets,
            import_inference_presets_dialog,
            export_model_definition,
            preview_model_definition_import,
            import_model_definition,
            preview_modelfile_import,
            list_ollama_models,
            preview_ollama_import,
            check_app_update,
            install_app_update,
            get_app_update_progress,
            get_launch_at_login,
            set_launch_at_login,
            get_control_api_status,
            set_control_api_settings,
            get_control_api_token,
            list_api_tokens,
            create_api_token,
            update_api_token,
            revoke_api_token,
            get_otel_settings,
            save_otel_settings,
            test_otel_export,
            get_gateway_status,
            set_gateway_settings,
            unload_gateway_model,
            queue_gateway_load_override,
            clear_gateway_responses,
            gateway_co_residency,
            start_benchmark,
            benchmark_preflight,
            get_benchmark_status,
            cancel_benchmark,
            list_benchmark_runs,
            delete_benchmark_run,
            clear_benchmark_runs,
            scan_local_models,
            inspect_model_paths,
            get_model_metadata,
            delete_local_model,
            list_library,
            set_model_favorite,
            reveal_model_in_folder,
            get_model_delete_preview,
            delete_library_model,
            find_duplicate_models,
            verify_duplicate_models,
            repair_model_shards,
            suggest_draft_models,
            suggest_projector,
            get_model_preset,
            get_mcp_config,
            save_mcp_config,
            read_mcp_import_source,
            get_mcp_import_sources,
            scan_import_sources,
            import_models,
            unlink_external_model,
            save_model_preset,
            search_hf_models,
            search_hf_models_page,
            get_hf_model_variants,
            download_image_companions,
            get_image_bundle_plan,
            sync_image_definitions,
            ensure_library_definitions,
            save_generated_image,
            get_hf_avatars,
            download_hf_variant,
            get_download_progress,
            cancel_active_download,
            list_downloads,
            resume_download,
            remove_download,
            clear_finished_downloads,
            prioritize_download,
            get_hf_model_card,
            check_hf_access,
            download_custom_url,
            check_model_artifact_update,
            get_telemetry_snapshot,
            watch_telemetry,
            get_telemetry_all,
            get_telemetry_history,
            get_perf_details,
            check_offline_crash,
            save_crash_event,
            list_diagnostic_reports,
            clear_diagnostic_reports,
            get_storage_directories,
            add_storage_directory,
            remove_storage_directory,
            set_primary_storage_directory,
            migrate_model,
            pick_directory_dialog,
            list_server_log_files,
            read_server_log_file,
            search_server_logs,
            export_server_logs,
            // Playground conversations
            get_playground_state,
            set_playground_state,
            import_playground_state,
            conversation_db_status,
            list_conversations,
            create_conversation,
            open_conversation,
            get_conversation,
            last_opened_conversation,
            set_conversation_title,
            update_conversation,
            set_conversation_pinned,
            set_conversation_favorite,
            move_conversation,
            delete_conversation,
            duplicate_conversation,
            fork_conversation,
            append_conversation_message,
            add_message_variant,
            update_message_variant,
            select_message_variant,
            select_conversation_branch,
            adopt_compare_side,
            edit_user_message,
            list_conversation_folders,
            create_conversation_folder,
            rename_conversation_folder,
            delete_conversation_folder,
            import_legacy_playground,
            export_conversation,
            export_diagram_svg,
            export_diagram_png,
            rewind_conversation_before,
            import_conversation_dialog,
            // Playground context compaction
            compact_conversation,
            cancel_compaction,
            set_compaction_undone,
            count_playground_tokens,
            // Playground documents (local RAG)
            rag_models,
            rag_list_documents,
            rag_pick_files,
            pick_image_files,
            pick_audio_files,
            read_audio_file,
            read_image_file,
            rag_add_files,
            rag_reindex,
            rag_cancel,
            rag_delete_document,
            rag_set_document_mode,
            rag_prepare,
            // Integrations
            detect_installed_clients,
            list_integrations,
            preview_integration,
            check_integration_alias_drift,
            check_integration_endpoint_drift,
            apply_integration,
            revert_integration,
            test_integration_connection,
            launch_tools,
            launch_integration_tool,
            opencode_desktop_apps,
            open_opencode_desktop,
            list_agent_models,
            check_model_tool_calling,
            // Harness MCP
            harness_mcp_overview,
            cli_status,
            cli_path_install,
            cli_path_uninstall,
            preview_harness_mcp,
            apply_harness_mcp,
            remove_harness_mcp,
            tools_list,
            tools_recover_text_calls,
            tools_inventory,
            generator_token_estimate,
            auto_tools_state,
            auto_tools_start,
            auto_tools_action,
            tools_decision,
            tools_run,
            tools_deny,
            tools_cancel,
            tools_get_settings,
            tools_set_settings,
            tools_get_search_provider,
            tools_set_search_provider,
            tools_log,
            installed_tools_list,
            installed_tools_files,
            installed_tools_inspect,
            installed_tools_install,
            installed_tools_remove,
            installed_tools_set_enabled,
            installed_tools_test,
            generated_tool_generate,
            generated_tool_generate_observed,
            generated_tool_cancel,
            generated_tool_approve,
            generated_tool_discard,
            generated_tool_save,
            generated_tool_remove,
            generated_tools_session,
            generated_tool_model_params,
            tools_mcp_status,
            tools_mcp_resources,
            tools_mcp_resource_to_file,
            tools_mcp_authorize,
            tools_mcp_cancel_authorize,
            tools_mcp_logout,
            tools_set_mcp_class,
            get_analytics_summary,
            record_direct_usage,
            list_analytics_requests,
            get_analytics_request,
            get_analytics_info,
            set_analytics_settings,
            clear_analytics,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |app, event| {
            // macOS: clicking the Dock icon with the window hidden in the menu bar (or its
            // web view released) brings it back, like the menu bar icon does.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = &event
            {
                desktop::show_main_window(app);
            }
            // The window's web view was released to the tray on purpose: the app keeps
            // running without a window until the tray shows it again (or "Quit").
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = &event
            {
                if desktop::window_released() {
                    api.prevent_exit();
                }
            }
            // Stop every llama-server on a normal exit (on Windows the job object
            // also covers crashes; on Unix this is the main cleanup path).
            if let tauri::RunEvent::Exit = event {
                let registry = app.state::<Arc<ServerRegistry>>().inner().clone();
                tauri::async_runtime::block_on(registry.stop_all());
                // The Lemonade engine's lemond and the backends it started.
                harness_core::lemonade::shutdown_now();
                tauri::async_runtime::block_on(exit_gateway.stop());
                // Requests still waiting for their telemetry sample are written now.
                analytics::flush(std::time::Duration::from_secs(3));
                // Spans and metrics still queued are sent now (bounded by the export timeout).
                tauri::async_runtime::block_on(harness_core::otel::shutdown());
                tauri::async_runtime::block_on(exit_api.stop());
                tauri::async_runtime::block_on(stop_mcp());
                if let Some(state) = app.try_state::<InstanceState>() {
                    if let Some(lock) = &state.lock {
                        lock.release();
                    }
                }
                #[cfg(target_os = "linux")]
                app_update::relaunch_appimage_after_exit();
            }
        });
}
