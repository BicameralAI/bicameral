use anyhow::{anyhow, Context};
use bicameral_api::hosted_bridge::{
    HostedBridgeErrorResponse, LocalPairingCodeLookupRequest, LocalPairingDecisionRequest,
    LocalPairingDecisionResponse, PairingDecisionAction, PendingPairingListResponse,
    PendingPairingLocalView,
};
use bicameral_config::{BicameralConfig, WorkspaceDiscovery};
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::time::Duration;

pub async fn pending(workspace: Option<&Path>) -> anyhow::Result<()> {
    let base_url = gateway_base_url(workspace)?;
    let client = reqwest::Client::new();
    let requests = fetch_pending(&client, &base_url).await?;
    if requests.requests.is_empty() {
        println!("No pending hosted pairing requests.");
        return Ok(());
    }

    println!("Pending hosted pairing requests:");
    for request in requests.requests {
        println!(
            "  {}  {}  expires {}",
            request.requesting_origin, request.cloud_account_id, request.expires_at
        );
    }
    Ok(())
}

pub async fn decide(action: PairingDecisionAction, workspace: Option<&Path>) -> anyhow::Result<()> {
    let base_url = gateway_base_url(workspace)?;
    let code = tokio::task::spawn_blocking(|| read_hidden("Paste the code shown in the browser: "))
        .await
        .context("pairing code prompt failed")??;
    let client = reqwest::Client::new();
    lookup_confirm_and_decide(&client, &base_url, &code, action).await?;
    Ok(())
}

/// Spawn the foreground gateway's terminal confirmation adapter.
///
/// It polls only the LOCAL summary endpoint, prompts once per observed request,
/// and never prints the browser-displayed code. HTTP serving continues on a
/// separate task while terminal input blocks.
pub fn spawn_foreground_watcher(base_url: String) -> Option<tokio::task::JoinHandle<()>> {
    if !io::stdin().is_terminal() {
        println!(
            "Hosted pairing confirmation is available with `bicameral pairing pending` and `bicameral pairing approve`."
        );
        return None;
    }

    Some(tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut handled = HashSet::new();
        loop {
            match fetch_pending(&client, &base_url).await {
                Ok(pending) => {
                    for request in pending.requests {
                        if handled.contains(&request.request_handle) {
                            continue;
                        }
                        println!("\nA hosted browser is waiting to pair.");
                        let code = match tokio::task::spawn_blocking(|| {
                            read_hidden("Paste the code shown in the browser: ")
                        })
                        .await
                        {
                            Ok(Ok(code)) => code,
                            Ok(Err(error)) => {
                                eprintln!("Could not read pairing code: {error}");
                                handled.insert(request.request_handle.clone());
                                continue;
                            }
                            Err(error) => {
                                eprintln!("Pairing prompt failed: {error}");
                                handled.insert(request.request_handle.clone());
                                continue;
                            }
                        };

                        if code.trim().is_empty() {
                            println!("Pairing left pending; no code was entered.");
                            handled.insert(request.request_handle.clone());
                            continue;
                        }

                        match lookup_confirm_and_decide(
                            &client,
                            &base_url,
                            &code,
                            PairingDecisionAction::Approve,
                        )
                        .await
                        {
                            Ok(matched_handle) => {
                                handled.insert(matched_handle);
                            }
                            Err(error) => {
                                eprintln!("Pairing was not approved: {error}");
                                handled.insert(request.request_handle);
                            }
                        }
                        // Re-fetch after each entry: the code may have matched a
                        // different request than the one that triggered this
                        // prompt, and a decision changes the pending set.
                        break;
                    }
                }
                Err(_) => {
                    // Normal during gateway startup and shutdown. Keep the
                    // foreground log quiet until a request can be observed.
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }))
}

fn gateway_base_url(workspace: Option<&Path>) -> anyhow::Result<String> {
    let config = match workspace {
        Some(start) => WorkspaceDiscovery::discover(start)?.1,
        None => {
            let current = std::env::current_dir()?;
            WorkspaceDiscovery::discover(&current)
                .map(|(_, config)| config)
                .unwrap_or_else(|_| BicameralConfig::default())
        }
    };
    let host = if config.gateway.host == "::1" {
        "[::1]".to_string()
    } else {
        config.gateway.host
    };
    Ok(format!("http://{}:{}", host, config.gateway.port))
}

async fn fetch_pending(
    client: &reqwest::Client,
    base_url: &str,
) -> anyhow::Result<PendingPairingListResponse> {
    let response = client
        .get(format!("{base_url}/api/v1/hosted-bridge/pair/pending"))
        .send()
        .await
        .context("local Bicameral gateway is not reachable")?;
    parse_response(response).await
}

async fn lookup_confirm_and_decide(
    client: &reqwest::Client,
    base_url: &str,
    code: &str,
    requested_action: PairingDecisionAction,
) -> anyhow::Result<String> {
    let lookup = client
        .post(format!("{base_url}/api/v1/hosted-bridge/pair/code/lookup"))
        .json(&LocalPairingCodeLookupRequest {
            pairing_code: code.to_string(),
        })
        .send()
        .await
        .context("local Bicameral gateway is not reachable")?;
    let view: PendingPairingLocalView = parse_response(lookup).await?;
    let matched_handle = view.request_handle.clone();

    println!("Pairing request matched:");
    println!("  Origin:  {}", view.requesting_origin);
    println!("  Account: {}", view.cloud_account_id);
    println!("  Expires: {}", view.expires_at);
    println!("  Allowed:");
    for capability in &view.capability_summary.allowed {
        println!("    - {capability}");
    }
    println!("  Forbidden:");
    for capability in &view.capability_summary.forbidden {
        println!("    - {capability}");
    }

    let (prompt, action) = match requested_action {
        PairingDecisionAction::Approve => ("Approve? [y/N] ", PairingDecisionAction::Approve),
        PairingDecisionAction::Deny => ("Deny? [y/N] ", PairingDecisionAction::Deny),
    };
    if !confirm(prompt)? {
        if requested_action == PairingDecisionAction::Approve {
            println!("Pairing not approved; request remains pending until it expires.");
        } else {
            println!("Pairing not denied; request remains pending until it expires.");
        }
        return Ok(matched_handle);
    }

    let response = client
        .post(format!("{base_url}/api/v1/hosted-bridge/pair/decision"))
        .json(&LocalPairingDecisionRequest {
            request_handle: view.request_handle,
            action,
            local_confirmation_nonce: view.local_confirmation_nonce,
        })
        .send()
        .await
        .context("local Bicameral gateway is not reachable")?;
    let result: LocalPairingDecisionResponse = parse_response(response).await?;
    println!("{}", result.message);
    Ok(matched_handle)
}

async fn parse_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> anyhow::Result<T> {
    let status = response.status();
    let bytes = response.bytes().await?;
    if !status.is_success() {
        if let Ok(error) = serde_json::from_slice::<HostedBridgeErrorResponse>(&bytes) {
            return Err(anyhow!(error.message));
        }
        return Err(anyhow!(
            "gateway returned HTTP {}: {}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }
    serde_json::from_slice(&bytes).context("gateway returned an invalid pairing response")
}

fn confirm(prompt: &str) -> anyhow::Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(unix)]
fn read_hidden(prompt: &str) -> anyhow::Result<String> {
    if !io::stdin().is_terminal() {
        return Err(anyhow!(
            "pairing code entry requires an interactive terminal"
        ));
    }
    print!("{prompt}");
    io::stdout().flush()?;

    #[cfg(unix)]
    let original = set_terminal_echo(false)?;

    let mut input = String::new();
    let read_result = io::stdin().read_line(&mut input);

    #[cfg(unix)]
    {
        let restore_result = restore_terminal(original);
        println!();
        restore_result?;
    }

    read_result?;
    Ok(input.trim().to_string())
}

#[cfg(unix)]
fn set_terminal_echo(enabled: bool) -> anyhow::Result<libc::termios> {
    let fd = libc::STDIN_FILENO;
    let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
    // SAFETY: `termios` points to valid writable storage and `fd` is stdin.
    if unsafe { libc::tcgetattr(fd, termios.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: tcgetattr succeeded and initialized the value.
    let original = unsafe { termios.assume_init() };
    let mut updated = original;
    if enabled {
        updated.c_lflag |= libc::ECHO;
    } else {
        updated.c_lflag &= !libc::ECHO;
    }
    // SAFETY: `updated` is a valid termios value for stdin.
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &updated) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(original)
}

#[cfg(unix)]
fn restore_terminal(original: libc::termios) -> anyhow::Result<()> {
    // SAFETY: `original` came from tcgetattr for stdin in this process.
    if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &original) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(not(unix))]
fn read_hidden(_prompt: &str) -> anyhow::Result<String> {
    Err(anyhow!(
        "hidden pairing-code entry is not yet supported on this platform"
    ))
}
