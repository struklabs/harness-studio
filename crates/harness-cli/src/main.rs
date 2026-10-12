//! `harness`: the command-line interface and headless mode of Harness Studio.
//!
//! Every command talks to a Control API (see [`session`]): the running instance's, a remote
//! one (`--host`), or a temporary in-process one that owns the data directory for the
//! duration of the command. stdout carries data (`--json`: the API's JSON), stderr progress,
//! warnings and errors. Exit codes: [`error::Exit`].

mod chat;
mod cli;
mod client;
mod commands;
mod doctor;
mod error;
mod image;
mod launch;
mod mcp;
mod messages;
mod output;
mod serve;
mod service;
mod session;

use clap::Parser;

use crate::error::{CliError, Exit};
use crate::output::Out;

fn main() {
    let json_requested = std::env::args().any(|a| a == "--json");
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            use clap::error::ErrorKind;
            if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) {
                let _ = e.print();
                std::process::exit(0);
            }
            if json_requested {
                let err = CliError::usage("cli.usage", e.to_string().trim().to_string());
                eprintln!(
                    "{}",
                    serde_json::to_string(&err.to_json()).unwrap_or_default()
                );
            } else {
                let _ = e.print();
            }
            std::process::exit(Exit::Usage.code());
        }
    };
    let out = Out {
        json: cli.global.json,
        quiet: cli.global.quiet,
        yes: cli.global.yes,
    };
    let code = match run(cli, &out) {
        // `harness launch` exits with the tool's exit code.
        Ok(()) => launch::CHILD_EXIT.load(std::sync::atomic::Ordering::SeqCst),
        Err(e) => {
            out.error(&e);
            e.exit.code()
        }
    };
    std::process::exit(code);
}

fn run(cli: cli::Cli, out: &Out) -> Result<(), CliError> {
    // Needs no data directory.
    if let cli::Command::Completion { shell } = &cli.command {
        commands::completion(*shell);
        return Ok(());
    }
    // Needs no data directory either (the uninstaller runs it).
    if let cli::Command::RemoveFromPath = &cli.command {
        let exe =
            std::env::current_exe().map_err(|e| CliError::general("cli.io", e.to_string()))?;
        return harness_core::integrations::cli_install::uninstall(&exe)
            .map_err(|e| CliError::general("cli.io", e));
    }
    // Isolated runs (tests, CI): never touch the OS keychain.
    if std::env::var_os("HARNESS_MEMORY_SECRETS").is_some_and(|v| v == "1") {
        harness_core::secrets::use_memory_store();
    }
    let data_dir = session::resolve_data_dir(&cli.global)?;
    harness_core::paths::init(data_dir.clone());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    // The command futures are large; the main thread's 1 MB stack (Windows) is not enough for a
    // debug build, so the runtime is driven from a thread with room.
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("harness-cli".into())
            .stack_size(32 * 1024 * 1024)
            .spawn_scoped(scope, || {
                runtime.block_on(commands::dispatch(cli, &data_dir, out))
            })
            .map_err(|e| CliError::general("cli.io", e.to_string()))?
            .join()
            .unwrap_or_else(|_| Err(CliError::general("cli.io", "the command panicked")))
    })
}
