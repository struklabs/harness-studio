//! Command dispatch.

pub mod admin;
pub mod analytics;
pub mod asr;
pub mod conversations;
pub mod convert;
pub mod hf;
pub mod lemonade;
pub mod models;
pub mod resolve;
pub mod runtimes;
pub mod servers;
pub mod tokens;
pub mod tools;
pub mod tts;

use std::path::Path;

use crate::cli::{
    BenchmarkCommand, Cli, Command, ConversationCommand, ImportCommand, LoraCommand,
    ManagedCommand, OtelCommand, ProfileCommand, RouterCommand, RuntimeCommand, ServiceCommand,
    TokenCommand, ToolsCommand,
};
use crate::error::CliResult;
use crate::output::Out;
use crate::session::{connect, Need, Session};

/// Which instance a command needs (`None` = none: local-only commands).
fn need(cmd: &Command) -> Option<Need> {
    use Command::*;
    Some(match cmd {
        Status
        | Doctor
        | Search { .. }
        | Serve(_)
        | Completion { .. }
        | Service { .. }
        | Hf { .. }
        | Speak(_)
        | Tts { .. }
        | Transcribe(_)
        | Asr { .. }
        | Lemonade { .. }
        | Convert(_)
        | Mcp(_) => return None,
        Load { .. } | Run(_) | Chat(_) | Image(_) => Need::Servers,
        Launch(a) if a.dry_run => Need::Library,
        Launch(_) => Need::Servers,
        Profile {
            command: ProfileCommand::Run { .. },
        } => Need::Servers,
        Unload { .. }
        | Stop { .. }
        | Logs { .. }
        | Shutdown
        | Retry { .. }
        | Lora { .. }
        | Router { .. } => Need::Existing,
        // The summary is requested through a running instance's gateway.
        Conversations {
            command: ConversationCommand::Compact { .. },
        } => Need::Existing,
        Pull { detach: true, .. } => Need::Existing,
        _ => Need::Library,
    })
}

pub async fn dispatch(cli: Cli, data_dir: &Path, out: &Out) -> CliResult<()> {
    let g = &cli.global;
    let Some(need) = need(&cli.command) else {
        return match &cli.command {
            Command::Status => crate::doctor::status(g, data_dir, out).await,
            Command::Doctor => crate::doctor::doctor(data_dir, out).await,
            Command::Search { query, limit, sort } => {
                models::search(query, *limit, sort.clone(), out).await
            }
            Command::Serve(args) => crate::serve::run(args, data_dir, out).await,
            Command::Mcp(args) => crate::mcp::run(args, g, data_dir).await,
            Command::Hf { command } => hf::run(command, out).await,
            Command::Speak(args) => tts::speak(args, out).await,
            Command::Tts { command } => tts::command(command, out).await,
            Command::Transcribe(args) => asr::transcribe(args, out).await,
            Command::Asr { command } => asr::command(command, out).await,
            Command::Lemonade { command } => Box::pin(lemonade::command(command, out)).await,
            Command::Convert(args) => Box::pin(convert::run(args, out)).await,
            Command::Completion { shell } => {
                completion(*shell);
                Ok(())
            }
            Command::RemoveFromPath => unreachable!("handled before the data directory"),
            Command::Service { command } => {
                let (action, args) = match command {
                    ServiceCommand::Install(a) => (crate::service::Action::Install, a),
                    ServiceCommand::Uninstall(a) => (crate::service::Action::Uninstall, a),
                    ServiceCommand::Print(a) => (crate::service::Action::Print, a),
                };
                crate::service::run(action, args, g.data_dir.as_deref(), data_dir, out)
            }
            _ => unreachable!("covered by need()"),
        };
    };
    let session = connect(g, data_dir, need, out).await?;
    let result = run_with(&session, cli.command, out).await;
    session.close().await;
    result
}

/// Prints a completion script (a closed pipe, e.g. `| head`, is not an error).
pub fn completion(shell: clap_complete::Shell) {
    use std::io::Write;
    let mut cmd = <Cli as clap::CommandFactory>::command();
    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut cmd, "harness", &mut buf);
    let _ = std::io::stdout().write_all(&buf);
}

async fn run_with(session: &Session, command: Command, out: &Out) -> CliResult<()> {
    let client = &session.client;
    match command {
        Command::Ls => models::ls(client, out).await,
        Command::Show { model } => models::show(client, &model, out).await,
        Command::Pull {
            model,
            file,
            detach,
        } => models::pull(client, &model, file, detach, out).await,
        Command::Rm { model, definitions } => models::rm(client, &model, definitions, out).await,
        Command::Ps { all } => servers::ps(client, all, out).await,
        Command::Analytics { range } => analytics::summary(client, &range, out).await,
        Command::Requests {
            id,
            model,
            client: by_client,
            status,
            tool_issue,
            tool_category,
            limit,
        } => {
            analytics::requests(
                client,
                analytics::RequestsArgs {
                    id,
                    model,
                    client: by_client,
                    status,
                    tool_issue,
                    tool_category,
                    limit,
                },
                out,
            )
            .await
        }
        Command::Conversations { command } => match command {
            ConversationCommand::Ls {
                query,
                folder,
                pinned,
                favorites,
                limit,
            } => {
                conversations::ls(
                    client,
                    conversations::ListArgs {
                        query,
                        folder,
                        pinned,
                        favorites,
                        limit,
                    },
                    out,
                )
                .await
            }
            ConversationCommand::Show { id } => conversations::show(client, &id, out).await,
            ConversationCommand::Export { id, format, output } => {
                conversations::export(client, &id, &format, output, out).await
            }
            ConversationCommand::Docs { id } => conversations::docs(client, &id, out).await,
            ConversationCommand::Compact {
                id,
                model,
                n_ctx,
                keep_turns,
                keep_tool_results,
                summary_tokens,
                instruction,
            } => {
                conversations::compact(
                    client,
                    conversations::CompactArgs {
                        id,
                        model,
                        n_ctx,
                        keep_turns,
                        keep_tool_results,
                        summary_tokens,
                        instruction,
                    },
                    out,
                )
                .await
            }
            ConversationCommand::Undo { id, marker, redo } => {
                conversations::undo(client, &id, &marker, redo, out).await
            }
        },
        Command::Load {
            model,
            server,
            no_wait,
            timeout,
        } => servers::load(session, &model, server, no_wait, timeout, out).await,
        Command::Unload { model, server } => servers::unload(client, model, server, out).await,
        Command::Retry {
            server,
            option,
            set,
            unload,
            save,
            no_wait,
            timeout,
        } => {
            servers::retry(
                client,
                servers::RetryArgs {
                    server,
                    option,
                    set,
                    unload,
                    save,
                    no_wait,
                    timeout,
                },
                out,
            )
            .await
        }
        Command::Stop { servers: ids, all } => servers::stop(client, ids, all, out).await,
        Command::Logs {
            server,
            follow,
            lines,
        } => servers::logs(client, &server, follow, lines, out).await,
        Command::Lora { command } => match command {
            LoraCommand::List { server } => servers::lora_list(client, &server, out).await,
            LoraCommand::Set { server, id, scale } => {
                servers::lora_set(client, &server, id, scale, out).await
            }
        },
        Command::Shutdown => servers::shutdown(session, out).await,
        Command::Run(args) => crate::chat::run(session, &args, out).await,
        Command::Chat(args) => crate::chat::chat(session, &args, out).await,
        Command::Image(args) => crate::image::run(session, &args, out).await,
        Command::Launch(args) => crate::launch::run(session, &args, out).await,
        Command::Import { command } => match command {
            ImportCommand::Modelfile {
                file,
                ollama,
                alias,
                dry_run,
            } => admin::import_modelfile(client, file, ollama, alias, dry_run, out).await,
        },
        Command::Profile { command } => match command {
            ProfileCommand::Ls => admin::profile_ls(client, out).await,
            ProfileCommand::Show { id } => admin::profile_show(client, &id, out).await,
            ProfileCommand::Run {
                id,
                no_wait,
                timeout,
            } => admin::profile_run(client, &id, no_wait, timeout, out).await,
            ProfileCommand::Import { file } => admin::profile_import(client, &file, out).await,
            ProfileCommand::Export { id, output } => {
                admin::profile_export(client, &id, output.as_deref(), out).await
            }
        },
        Command::Router { command } => match command {
            RouterCommand::Models { server } => servers::router_models(client, &server, out).await,
            RouterCommand::Load {
                server,
                model,
                no_wait,
                timeout,
            } => servers::router_load(client, &server, &model, no_wait, timeout, out).await,
            RouterCommand::Unload { server, model } => {
                servers::router_unload(client, &server, &model, out).await
            }
        },
        Command::Benchmark { command } => match command {
            BenchmarkCommand::Run {
                model,
                n_prompt,
                n_gen,
                ngl,
                repetitions,
                threads,
            } => {
                admin::benchmark_run(
                    client,
                    admin::BenchArgs {
                        model,
                        n_prompt,
                        n_gen,
                        ngl,
                        repetitions,
                        threads,
                    },
                    out,
                )
                .await
            }
            BenchmarkCommand::Ls => admin::benchmark_ls(client, out).await,
            BenchmarkCommand::Show { id } => admin::benchmark_show(client, &id, out).await,
        },
        Command::Runtime { command } => match command {
            RuntimeCommand::Ls => runtimes::ls(client, out).await,
            RuntimeCommand::Add {
                github,
                local,
                family,
                name,
                id,
                tag,
                asset,
                backend,
                allow_unverified,
                copy,
            } => {
                runtimes::add(
                    client,
                    runtimes::AddArgs {
                        github,
                        local,
                        family,
                        name,
                        id,
                        tag,
                        asset,
                        backend,
                        allow_unverified,
                        copy,
                    },
                    out,
                )
                .await
            }
            RuntimeCommand::Rm { id, force } => runtimes::rm(client, &id, force, out).await,
            RuntimeCommand::Verify { id, accept_changes } => {
                runtimes::verify(client, &id, accept_changes, out).await
            }
            RuntimeCommand::Suggest { model } => runtimes::suggest(client, &model, out).await,
            RuntimeCommand::Managed { command } => match command {
                ManagedCommand::Status => runtimes::managed_status(client, out).await,
                ManagedCommand::Install {
                    backend,
                    allow_unverified,
                } => runtimes::managed_install(client, backend, allow_unverified, out).await,
                ManagedCommand::Rollback => runtimes::managed_rollback(client, out).await,
            },
        },
        Command::ApiToken { command } => match command {
            TokenCommand::Create {
                name,
                scopes,
                policy,
            } => tokens::token_create(client, &name, &scopes, &policy, out).await,
            TokenCommand::Ls => tokens::token_ls(client, out).await,
            TokenCommand::Show { id } => tokens::token_show(client, &id, out).await,
            TokenCommand::Set {
                id,
                name,
                scopes,
                policy,
            } => tokens::token_set(client, &id, name.as_deref(), &scopes, &policy, out).await,
            TokenCommand::Rm { id } => tokens::token_rm(client, &id, out).await,
        },
        Command::Tools { command } => match command {
            ToolsCommand::Inventory => tools::inventory(client, out).await,
            ToolsCommand::Auto { command } => tools::automatic(client, command, out).await,
            ToolsCommand::List => tools::list(client, out).await,
            ToolsCommand::Mcp { retry } => tools::mcp_status(client, retry, out).await,
            ToolsCommand::McpResources { server, read } => {
                tools::mcp_resources(client, &server, read.as_deref(), out).await
            }
            ToolsCommand::McpAuthorize { server } => {
                tools::mcp_authorize(client, &server, out).await
            }
            ToolsCommand::McpLogout { server } => tools::mcp_logout(client, &server, out).await,
            ToolsCommand::Run {
                name,
                args,
                workdir,
                timeout,
            } => tools::run(client, &name, &args, workdir, timeout, out).await,
            ToolsCommand::SearchProvider {
                provider,
                url,
                api_key,
            } => {
                tools::search_provider(
                    client,
                    provider.as_deref(),
                    url.as_deref(),
                    api_key.as_deref(),
                    out,
                )
                .await
            }
        },
        Command::Otel { command } => match command {
            OtelCommand::Status => tokens::otel_status(client, out).await,
            OtelCommand::Test { endpoint, protocol } => {
                tokens::otel_test(client, endpoint.as_deref(), protocol.as_deref(), out).await
            }
        },
        Command::Status
        | Command::Doctor
        | Command::Search { .. }
        | Command::Serve(_)
        | Command::Mcp(_)
        | Command::Completion { .. }
        | Command::RemoveFromPath
        | Command::Service { .. }
        | Command::Speak(_)
        | Command::Tts { .. }
        | Command::Transcribe(_)
        | Command::Asr { .. }
        | Command::Lemonade { .. }
        | Command::Convert(_)
        | Command::Hf { .. } => unreachable!("local commands"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use harness_core::instance::{InstanceKind, InstanceLock};

    #[tokio::test]
    async fn automatic_commands_refuse_a_gui_owner_without_api_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let _owner = InstanceLock::acquire(dir.path(), InstanceKind::Gui, "test").unwrap();
        let run = dir.path().join("auto-tools/run.json");
        std::fs::create_dir_all(run.parent().unwrap()).unwrap();
        std::fs::write(&run, b"saved run").unwrap();
        let out = Out {
            json: true,
            quiet: true,
            yes: true,
        };
        for args in [
            vec!["status"],
            vec!["action", "pause"],
            vec!["action", "stop"],
            vec![
                "start",
                "--endpoint",
                "http://localhost:1234/v1",
                "--count",
                "1",
            ],
        ] {
            let mut cli_args = vec!["harness", "tools", "auto"];
            cli_args.extend(args);
            let mut cli = Cli::try_parse_from(cli_args).unwrap();
            // Keep ambient remote connection variables out of this local ownership test.
            cli.global.host = None;
            cli.global.token = None;
            assert!(matches!(need(&cli.command), Some(Need::Library)));
            let e = dispatch(cli, dir.path(), &out).await.unwrap_err();
            assert_eq!(e.code, "cli.owner_without_api");
            assert_eq!(std::fs::read(&run).unwrap(), b"saved run");
        }
    }
    #[tokio::test]
    async fn automatic_action_uses_the_gui_owners_api_without_local_writes() {
        use harness_core::instance::ApiEndpoint;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let owner = InstanceLock::acquire(dir.path(), InstanceKind::Gui, "test").unwrap();
        let token = dir.path().join("test-token");
        std::fs::write(&token, "test-owner-token").unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        owner
            .set_api(
                Some(ApiEndpoint {
                    host: "127.0.0.1".into(),
                    port: listener.local_addr().unwrap().port(),
                }),
                Some(token),
            )
            .unwrap();
        let run = dir.path().join("auto-tools/run.json");
        std::fs::create_dir_all(run.parent().unwrap()).unwrap();
        std::fs::write(&run, b"owner run").unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 2048];
                let n = stream.read(&mut chunk).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&chunk[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let len: usize = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.starts_with("POST /api/harness/v1/tools/automatic/action "));
            assert!(request
                .to_lowercase()
                .contains("authorization: bearer test-owner-token"));
            assert!(request.contains("\"action\":\"stop\""));
            stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap();
        });
        let mut cli = Cli::try_parse_from(["harness", "tools", "auto", "action", "stop"]).unwrap();
        cli.global.host = None;
        cli.global.token = None;
        let out = Out {
            json: true,
            quiet: true,
            yes: true,
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(3),
            dispatch(cli, dir.path(), &out),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
        assert_eq!(std::fs::read(&run).unwrap(), b"owner run");
    }
}
