//! `harness hf login|logout|whoami`: the Hugging Face account used for Hub search, gated /
//! private repos and downloads. No instance is needed: the token lives in the OS keychain,
//! which the GUI and `harness serve` read too.

use std::io::{BufRead, IsTerminal, Read};

use harness_core::hf_auth::{self, HfAuthKind};
use serde_json::{json, Value};

use crate::cli::HfCommand;
use crate::error::{CliError, CliResult};
use crate::output::Out;

pub async fn run(command: &HfCommand, out: &Out) -> CliResult<()> {
    match command {
        HfCommand::Login {
            with_token,
            device,
            client_id,
            no_browser,
        } => {
            if *with_token {
                login_token(out).await
            } else {
                login_oauth(client_id.clone(), *device, *no_browser, out).await
            }
        }
        HfCommand::Logout => logout(out).await,
        HfCommand::Whoami => whoami(out).await,
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> CliResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| CliError::general("cli.io", e.to_string()))?
        .map_err(CliError::core)
}

/// Reads the token from stdin: a pipe/file (`harness hf login --with-token < token.txt`) or one line
/// typed at the prompt.
fn read_token(out: &Out) -> CliResult<String> {
    let stdin = std::io::stdin();
    let mut text = String::new();
    if stdin.is_terminal() {
        out.info("Paste a Hugging Face access token (https://huggingface.co/settings/tokens) and press Enter:");
        stdin
            .lock()
            .read_line(&mut text)
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    } else {
        stdin
            .lock()
            .read_to_string(&mut text)
            .map_err(|e| CliError::general("cli.io", e.to_string()))?;
    }
    Ok(text.trim().to_string())
}

fn user_json(user: &hf_auth::HfUser, kind: &str) -> Value {
    json!({ "kind": kind, "user": user })
}

async fn login_token(out: &Out) -> CliResult<()> {
    let token = read_token(out)?;
    let user = hf_auth::login_with_token(&token)
        .await
        .map_err(CliError::core)?;
    out.data(&user_json(&user, "token"), || {
        format!("Logged in to Hugging Face as {} (access token).", user.name)
    });
    Ok(())
}

async fn login_oauth(
    client_id: Option<String>,
    device: bool,
    no_browser: bool,
    out: &Out,
) -> CliResult<()> {
    let client_id = match client_id.map(|c| c.trim().to_string()) {
        Some(c) if hf_auth::valid_client_id(&c) => c,
        Some(_) => return Err(CliError::core("hf_auth.invalid_client_id")),
        None => {
            let settings = blocking(|| Ok(harness_core::config::AppSettings::load())).await?;
            match hf_auth::client_id(&settings) {
                Some((id, _)) => id,
                None => {
                    out.info(format!(
                        "Register a public OAuth app (no client secret) at \
https://huggingface.co/settings/applications/new with the redirect URI {} and the scopes {}, \
then pass --client-id or set {}. Or save an access token: `harness hf login --with-token`.",
                        hf_auth::REGISTERED_REDIRECT_URI,
                        hf_auth::SCOPES,
                        hf_auth::CLIENT_ID_ENV
                    ));
                    return Err(CliError::core("hf_auth.no_client_id"));
                }
            }
        }
    };

    let status = if device {
        let code = hf_auth::device_start(&client_id)
            .await
            .map_err(CliError::core)?;
        let open = code
            .verification_uri_complete
            .clone()
            .unwrap_or_else(|| code.verification_uri.clone());
        out.info(format!(
            "Open {} and enter the code {} (valid for {} min).",
            code.verification_uri,
            code.user_code,
            code.expires_in.div_ceil(60)
        ));
        if !no_browser && hf_auth::open_in_browser(&open).is_err() {
            out.warn("could not open a browser; open the address above yourself");
        }
        out.info("Waiting for approval... (Ctrl+C to stop)");
        hf_auth::device_finish(&client_id, &code)
            .await
            .map_err(CliError::core)?
    } else {
        hf_auth::login_with_browser(&client_id, |url| {
            out.info("Sign in with Hugging Face in your browser. If it does not open, visit:");
            out.info(url);
            if !no_browser && hf_auth::open_in_browser(url).is_err() {
                out.warn("could not open a browser; open the address above yourself");
            }
            out.info("Waiting for the browser... (Ctrl+C to stop)");
            Ok(())
        })
        .await
        .map_err(CliError::core)?
    };
    let value = serde_json::to_value(&status).unwrap_or(Value::Null);
    out.data(&value, || match &status.user {
        Some(u) => format!("Logged in to Hugging Face as {}.", u.name),
        None => "Logged in to Hugging Face.".into(),
    });
    Ok(())
}

async fn logout(out: &Out) -> CliResult<()> {
    let before = blocking(hf_auth::status).await?;
    blocking(hf_auth::logout).await?;
    let revoke = match before.kind {
        HfAuthKind::Oauth => Some(hf_auth::CONNECTED_APPS_URL),
        HfAuthKind::Token => Some(hf_auth::TOKENS_URL),
        HfAuthKind::None => None,
    };
    out.data(
        &json!({ "logged_out": before.kind != HfAuthKind::None, "revoke_at": revoke }),
        || match revoke {
            Some(url) => format!(
                "Logged out: the token was removed from the OS keychain.\n\
The grant stays valid on huggingface.co until you revoke it: {url}"
            ),
            None => "Not logged in.".into(),
        },
    );
    Ok(())
}

async fn whoami(out: &Out) -> CliResult<()> {
    let status = blocking(hf_auth::status).await?;
    if status.kind == HfAuthKind::None {
        return Err(CliError::core("hf_auth.not_logged_in"));
    }
    let token = hf_auth::current_token()
        .await
        .ok_or_else(|| CliError::core("hf_auth.not_logged_in"))?;
    // Checked online so an expired / revoked token shows up here.
    let user = hf_auth::whoami(&token).await.map_err(CliError::core)?;
    let kind = match status.kind {
        HfAuthKind::Oauth => "oauth",
        _ => "token",
    };
    let value = json!({
        "kind": kind,
        "user": user,
        "expires_at": status.expires_at,
        "scope": status.scope,
    });
    out.data(&value, || {
        let mut lines = vec![match &user.fullname {
            Some(full) => format!("{} ({})", user.name, full),
            None => user.name.clone(),
        }];
        lines.push(match status.kind {
            HfAuthKind::Oauth => "signed in with Hugging Face (OAuth)".to_string(),
            _ => "access token".to_string(),
        });
        if let Some(scope) = &status.scope {
            lines.push(format!("scopes: {scope}"));
        }
        lines.join("\n")
    });
    Ok(())
}
