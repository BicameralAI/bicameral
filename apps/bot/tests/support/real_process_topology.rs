//! Disposable real-process topology harness for the #789 alpha recall path.
//!
//! This harness proves the **bot portion** of the narrowed alpha recall
//! topology against a *production* daemon process, not an in-process or
//! fake-daemon substitute:
//!
//! ```text
//! coding agent
//!   -> MCP tool call                 (downstream #820: real MCP stdio client)
//!   -> production MCP stdio process   (downstream #820)
//!   -> ToolRequest over loopback HTTP (this harness: POST /v2/tool-requests)
//!   -> production bot daemon          (this harness: spawned `bicameral gateway start`)
//!   -> replay accepted Decisions + inspect local workspace/graph
//!   -> bot ToolResponse.result        (canonical nested result)
//!   -> MCP-rendered result            (downstream #820)
//! ```
//!
//! Boundaries (matching issue #819 acceptance criteria and non-goals):
//!
//! - The bot event store is the only authoritative product state. The
//!   [`TopologyReceipt`] and the [`RuntimeTrace`] carried on each response are
//!   **validation witnesses only** — never product state. The receipt records
//!   `authoritative: false` to make that explicit.
//! - Only disposable, controlled data is used: an ephemeral temp workspace, a
//!   default (git) event store under that temp dir, and synthetic seed
//!   Decisions. No production credentials, customer data, or persistent
//!   developer stores are required or touched.
//! - MCP is a request/render surface over loopback daemon communication. The
//!   real MCP stdio client is supplied by `bicameral-mcp` for the downstream
//!   lifecycle/UserJourney validation (#820); this harness exercises the
//!   loopback HTTP boundary the stdio client renders over and documents the
//!   exact request/response contract it must attach to.
//!
//! The harness also exposes crash/retry controls
//! ([`DisposableTopology::crash_daemon`], [`DisposableTopology::restart_daemon`],
//! [`DisposableTopology::daemon_running`]) and negative-path probes
//! ([`DisposableTopology::dispatch_raw`],
//! [`DisposableTopology::expect_daemon_unavailable`],
//! [`negotiate_client_startup`]) so #820 can drive protocol-drift,
//! daemon-unavailable, and shape-mismatch cases visibly.

#![allow(dead_code)]

use std::fs::File;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use bicameral_api::tool_request::{
    AuthMethod, AuthorityContext, CheckpointHint, IngestEvidenceItem, IngestSubmitLocalParams,
    PreflightRunParams, ReviewTargetParams, ToolCommand, ToolRequest, ToolResponse,
    ToolResponseStatus,
};
use serde::Serialize;
use serde_json::{json, Value};

/// Stable schema id for the topology receipt witness.
pub const TOPOLOGY_RECEIPT_SCHEMA: &str = "alpha-recall-real-process-topology-receipt/v1";

/// Protocol version the current bot daemon speaks. Kept in sync with
/// `bicameral-runtime`'s `PROTOCOL_VERSION`; used only for client-side
/// startup-negotiation modeling in the harness (mirrors the thin-client
/// conformance test), never as authority.
pub const DAEMON_PROTOCOL_VERSION: &str = "v2";

/// A single traversed process boundary, recorded as a validation witness.
#[derive(Debug, Clone, Serialize)]
pub struct TopologyHop {
    /// Which boundary this hop crossed, e.g. `mcp_render_surface -> loopback_http`.
    pub boundary: &'static str,
    /// Canonical daemon command name.
    pub command: String,
    /// Correlates with the daemon's `ToolResponse.request_id` / `RuntimeTrace`.
    pub request_id: String,
    /// Daemon-reported response status.
    pub response_status: String,
    /// Whether the daemon attached a `RuntimeTrace` witness on this hop.
    pub trace_present: bool,
    /// The daemon-reported `caller_surface` from the trace, when present.
    pub trace_caller_surface: Option<String>,
}

/// Validation witness describing the real-process topology a run exercised.
///
/// This is **not** product state. It exists only so downstream validation
/// (#820) and human reviewers can confirm the request actually crossed the
/// production process boundary (loopback HTTP -> spawned daemon) rather than an
/// in-process shortcut.
#[derive(Debug, Clone, Serialize)]
pub struct TopologyReceipt {
    pub schema: &'static str,
    /// Always `false`: the receipt is a witness, never authoritative product state.
    pub authoritative: bool,
    /// Always `true`: the topology uses disposable stores/data only.
    pub disposable: bool,
    pub loopback_addr: String,
    pub daemon_pid: u32,
    pub substrate: String,
    pub workspace_root: PathBuf,
    pub hops: Vec<TopologyHop>,
    pub witness_note: &'static str,
}

/// Error surface for harness operations, giving downstream validation typed,
/// visible failure cases (daemon unavailable, shape mismatch, etc.).
#[derive(Debug)]
pub enum TopologyError {
    /// The loopback daemon could not be reached (e.g. crashed / not started).
    DaemonUnavailable(String),
    /// The daemon returned a non-success HTTP status (e.g. 422 shape mismatch).
    HttpStatus { status: u16, body: String },
    /// Any other harness/transport error.
    Other(String),
}

impl std::fmt::Display for TopologyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TopologyError::DaemonUnavailable(m) => write!(f, "daemon_unavailable: {m}"),
            TopologyError::HttpStatus { status, body } => {
                write!(f, "http_status {status}: {body}")
            }
            TopologyError::Other(m) => write!(f, "topology_error: {m}"),
        }
    }
}

impl std::error::Error for TopologyError {}

/// Model a downstream thin-client startup negotiation against the daemon
/// protocol. Mirrors `bot_mcp_thin_client_conformance`'s `negotiate_startup`:
/// the client refuses to start on protocol drift rather than falling back to
/// legacy authority. Returned as a documented control for #820's protocol-drift
/// case.
pub fn negotiate_client_startup(
    client_supported_protocols: &[&str],
    daemon_protocol: &str,
) -> std::result::Result<(), String> {
    if client_supported_protocols.contains(&daemon_protocol) {
        Ok(())
    } else {
        Err(format!(
            "incompatible_protocol: daemon protocol '{daemon_protocol}' not in \
             client-supported {client_supported_protocols:?}; refusing startup \
             (no legacy handler fallback)"
        ))
    }
}

/// A running disposable real-process topology: a production `bicameral` daemon
/// bound to loopback in a throwaway workspace, plus a loopback HTTP client that
/// stands in for the MCP request/render surface.
pub struct DisposableTopology {
    workspace: tempfile::TempDir,
    port: u16,
    substrate: String,
    child: Option<Child>,
    client: reqwest::blocking::Client,
    hops: Vec<TopologyHop>,
}

impl DisposableTopology {
    /// Start a production daemon in a fresh disposable workspace on an ephemeral
    /// loopback port. The workspace is a real git repo seeded with a single
    /// fixture source file so the daemon's graph/symbol surface is exercised.
    pub fn start() -> Result<Self> {
        let workspace = tempfile::tempdir().context("create disposable workspace")?;
        let root = workspace.path();

        seed_git_workspace(root).context("seed disposable git workspace")?;
        run_bicameral(&["--workspace", &root.to_string_lossy(), "init"], root)
            .context("bicameral init")?;

        let port = free_loopback_port().context("reserve loopback port")?;
        set_gateway_port(root, port).context("pin gateway port")?;

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("build loopback client")?;

        let substrate = "git".to_string();

        let mut topology = Self {
            workspace,
            port,
            substrate,
            child: None,
            client,
            hops: Vec::new(),
        };
        topology.spawn_daemon().context("spawn production daemon")?;
        topology.await_ready().context("await daemon readiness")?;
        Ok(topology)
    }

    fn workspace_root(&self) -> &Path {
        self.workspace.path()
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn spawn_daemon(&mut self) -> Result<()> {
        let root = self.workspace.path().to_path_buf();
        let log_path = root.join(".bicameral").join("daemon.log");
        let log = File::create(&log_path).context("create daemon log")?;
        let err = log.try_clone().context("clone daemon log handle")?;
        let child = Command::new(env!("CARGO_BIN_EXE_bicameral"))
            .args(["--workspace", &root.to_string_lossy(), "gateway", "start"])
            .stdout(log)
            .stderr(err)
            .spawn()
            .context("spawn `bicameral gateway start`")?;
        self.child = Some(child);
        Ok(())
    }

    /// Poll `/health` until the daemon accepts loopback traffic, or time out.
    fn await_ready(&self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(30);
        let url = format!("{}/health", self.base_url());
        while Instant::now() < deadline {
            if let Ok(resp) = self.client.get(&url).send() {
                if resp.status().is_success() {
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        bail!(
            "daemon did not become ready on {}; log:\n{}",
            self.base_url(),
            self.daemon_log()
        )
    }

    /// Read the daemon's captured log (best effort) for diagnostics.
    pub fn daemon_log(&self) -> String {
        std::fs::read_to_string(self.workspace.path().join(".bicameral").join("daemon.log"))
            .unwrap_or_default()
    }

    /// GET the daemon capability report over loopback.
    pub fn capabilities(&self) -> Result<Value> {
        let url = format!("{}/v2/capabilities", self.base_url());
        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| TopologyError::DaemonUnavailable(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().unwrap_or_default();
            return Err(TopologyError::HttpStatus { status, body }.into());
        }
        resp.json::<Value>().context("parse capability report")
    }

    /// Dispatch a typed `ToolRequest` over the loopback HTTP boundary to the
    /// production daemon and return the canonical typed `ToolResponse`. Records
    /// a topology hop witness.
    pub fn dispatch(
        &mut self,
        request: &ToolRequest,
        boundary: &'static str,
    ) -> Result<ToolResponse> {
        let url = format!("{}/v2/tool-requests", self.base_url());
        let resp = self
            .client
            .post(&url)
            .json(request)
            .send()
            .map_err(|e| TopologyError::DaemonUnavailable(e.to_string()))?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().unwrap_or_default();
            return Err(TopologyError::HttpStatus { status, body }.into());
        }
        let response: ToolResponse = resp.json().context("parse ToolResponse")?;
        self.record_hop(boundary, &response);
        Ok(response)
    }

    fn record_hop(&mut self, boundary: &'static str, response: &ToolResponse) {
        self.hops.push(TopologyHop {
            boundary,
            command: response
                .trace
                .as_ref()
                .map(|t| t.command.clone())
                .unwrap_or_default(),
            request_id: response.request_id.to_string(),
            response_status: format!("{:?}", response.status),
            trace_present: response.trace.is_some(),
            trace_caller_surface: response.trace.as_ref().map(|t| t.caller_surface.clone()),
        });
    }

    /// POST a raw (possibly malformed) body to the ToolRequest boundary and
    /// return the `(http_status, body)`. This lets #820 exercise the
    /// shape-mismatch case visibly (the daemon rejects with 4xx rather than
    /// crashing or silently accepting).
    pub fn dispatch_raw(&self, raw_body: &str) -> Result<(u16, String)> {
        let url = format!("{}/v2/tool-requests", self.base_url());
        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(raw_body.to_string())
            .send()
            .map_err(|e| TopologyError::DaemonUnavailable(e.to_string()))?;
        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        Ok((status, body))
    }

    /// Assert the daemon is currently unreachable, returning the transport
    /// error. Used by the daemon-unavailable case after a crash / before start.
    pub fn expect_daemon_unavailable(&self, request: &ToolRequest) -> Result<TopologyError> {
        let url = format!("{}/v2/tool-requests", self.base_url());
        match self.client.post(&url).json(request).send() {
            Ok(resp) => bail!(
                "expected daemon-unavailable, but got HTTP {}",
                resp.status()
            ),
            Err(e) => Ok(TopologyError::DaemonUnavailable(e.to_string())),
        }
    }

    /// Crash the daemon process hard (SIGKILL), simulating an abrupt failure.
    /// Crash/retry control for downstream lifecycle validation (#820).
    pub fn crash_daemon(&mut self) -> Result<()> {
        if let Some(mut child) = self.child.take() {
            child.kill().context("kill daemon")?;
            child.wait().context("reap daemon")?;
        }
        // Wait until the loopback port stops accepting connections.
        let deadline = Instant::now() + Duration::from_secs(10);
        let url = format!("{}/health", self.base_url());
        while Instant::now() < deadline {
            if self.client.get(&url).send().is_err() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    }

    /// Restart the daemon against the same disposable workspace and port,
    /// proving durable recall from the disposable event store survives a crash.
    /// Crash/retry control for downstream lifecycle validation (#820).
    pub fn restart_daemon(&mut self) -> Result<()> {
        if self.child.is_some() {
            self.crash_daemon()?;
        }
        self.spawn_daemon()?;
        self.await_ready()
    }

    /// Whether the daemon child is still alive.
    pub fn daemon_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Seed one accepted Decision through the real loopback path (ingest ->
    /// accept), returning the accepted decision title for later assertions.
    pub fn seed_accepted_decision(
        &mut self,
        source_uri: &str,
        title: &str,
        description: &str,
    ) -> Result<()> {
        let ingest = ToolRequest::new(
            ToolCommand::IngestSubmitLocal(IngestSubmitLocalParams {
                source_uri: source_uri.into(),
                source_type: "meeting_transcript".into(),
                label: Some(title.into()),
                title: title.into(),
                description: description.into(),
                suggested_level: Some("architecture".into()),
                snapshot_content: Some(description.into()),
                evidence: vec![IngestEvidenceItem {
                    excerpt: description.into(),
                    span_start: Some(0),
                    span_end: Some(description.len()),
                    confidence: Some(0.95),
                }],
                candidate_drafts: Vec::new(),
                binding_hints: Vec::new(),
                rationale: Some("Issue #819 disposable topology seed".into()),
                metadata: None,
            }),
            authority("seed-agent", AuthMethod::McpSession, "mcp"),
        );
        let ingest_response = self.dispatch(&ingest, "mcp_render_surface -> loopback_http")?;
        if ingest_response.status != ToolResponseStatus::Ok {
            bail!("seed ingest failed: {:?}", ingest_response.message);
        }
        let candidate_id = ingest_response
            .result
            .as_ref()
            .and_then(|r| r.get("candidate_ids"))
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("seed ingest returned no candidate id"))?
            .to_string();

        let accept = ToolRequest::new(
            ToolCommand::ReviewAcceptCandidate(ReviewTargetParams {
                target_id: candidate_id,
                reason: Some("accepted for disposable topology proof".into()),
            }),
            authority("reviewer", AuthMethod::Dashboard, "dashboard"),
        );
        let accept_response =
            self.dispatch(&accept, "dashboard_render_surface -> loopback_http")?;
        if accept_response.status != ToolResponseStatus::Ok {
            bail!("seed accept failed: {:?}", accept_response.message);
        }
        Ok(())
    }

    /// Build a `preflight.run` recall request for the given files/symbols.
    pub fn preflight_request(&self, files: &[&str], symbols: &[&str]) -> ToolRequest {
        ToolRequest::new(
            ToolCommand::PreflightRun(PreflightRunParams {
                files: files.iter().map(|s| s.to_string()).collect(),
                symbols: symbols.iter().map(|s| s.to_string()).collect(),
                diff_context: None,
                branch: None,
                checkpoint_hint: Some(CheckpointHint::PreWork),
                managed_planning: None,
            }),
            authority("preflight-agent", AuthMethod::McpSession, "mcp"),
        )
    }

    /// The validation-witness receipt of the traversed process boundaries.
    pub fn topology_receipt(&self) -> TopologyReceipt {
        TopologyReceipt {
            schema: TOPOLOGY_RECEIPT_SCHEMA,
            authoritative: false,
            disposable: true,
            loopback_addr: format!("127.0.0.1:{}", self.port),
            daemon_pid: self.child.as_ref().map(Child::id).unwrap_or(0),
            substrate: self.substrate.clone(),
            workspace_root: self.workspace_root().to_path_buf(),
            hops: self.hops.clone(),
            witness_note:
                "Topology receipt and RuntimeTrace are validation witnesses only; the bot \
                 event store remains the sole authoritative product state.",
        }
    }
}

impl Drop for DisposableTopology {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // `workspace` (TempDir) removes the disposable stores on drop.
    }
}

fn authority(actor_id: &str, auth_method: AuthMethod, surface: &str) -> AuthorityContext {
    let mut audit_metadata = serde_json::Map::new();
    audit_metadata.insert("surface".into(), json!(surface));
    AuthorityContext {
        actor_id: actor_id.into(),
        auth_method,
        session_id: Some("issue-819-disposable-topology".into()),
        workspace: Some("/workspace".into()),
        policy_scope: vec!["default".into(), "review".into()],
        audit_metadata,
    }
}

/// Reserve an ephemeral loopback port by binding to `:0` and reading the
/// assigned port, then releasing it. The daemon config rejects port 0, so a
/// concrete port must be chosen.
fn free_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("bind ephemeral port")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Rewrite the workspace's gateway port to a chosen loopback port.
fn set_gateway_port(root: &Path, port: u16) -> Result<()> {
    let config_path = root.join(".bicameral").join("config.yaml");
    let content = std::fs::read_to_string(&config_path).context("read config.yaml")?;
    let rewritten = content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("port:") {
                let indent = &line[..line.len() - trimmed.len()];
                format!("{indent}port: {port}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&config_path, format!("{rewritten}\n")).context("write config.yaml")?;
    Ok(())
}

/// Seed a disposable git workspace with a single fixture source file so the
/// daemon has a real, committed workspace to resolve symbols against.
fn seed_git_workspace(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root.join("src/payments")).context("create fixture dir")?;
    std::fs::write(
        root.join("src/payments/idempotency.rs"),
        "pub fn apply_idempotency() {}\n",
    )
    .context("write fixture source")?;
    git(root, &["init", "-q"])?;
    git(root, &["add", "src"])?;
    git(
        root,
        &[
            "-c",
            "user.email=harness@example.invalid",
            "-c",
            "user.name=alpha-recall-harness",
            "commit",
            "-q",
            "-m",
            "seed disposable fixture",
        ],
    )?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| format!("run git {args:?}"))?;
    if !output.status.success() {
        bail!(
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn run_bicameral(args: &[&str], cwd: &Path) -> Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_bicameral"))
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("run bicameral {args:?}"))?;
    if !output.status.success() {
        bail!(
            "bicameral {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
