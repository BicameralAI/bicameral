use super::host_permissions::{
    self, DeclaredToolRegistry, HostPermissionCapabilities, HostPermissionPaths, PermissionPlan,
};
use anyhow::{bail, Context};
use bicameral_api::capability_readiness::{
    DeveloperHostId, HostPermissionObservation, McpDriftCode, McpPrerequisiteCode,
    McpReadinessObservation, VerifiedActivationEvidence,
};
use bicameral_config::{BicameralConfig, WorkspaceDiscovery};
use chrono::Utc;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DEFAULT_DOWNLOAD_BASE_URL: &str =
    "https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral";
const DOWNLOAD_BASE_ENV: &str = "BICAMERAL_DOWNLOAD_BASE";
const MANIFEST_PATH_ENV: &str = "BICAMERAL_RELEASE_MANIFEST";
const GATEWAY_PORT_ENV: &str = "BICAMERAL_GATEWAY_PORT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum DeveloperHost {
    Claude,
    Codex,
}

impl DeveloperHost {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    fn id(self) -> DeveloperHostId {
        match self {
            Self::Claude => DeveloperHostId::Claude,
            Self::Codex => DeveloperHostId::Codex,
        }
    }

    fn program(self) -> &'static str {
        self.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeveloperHostOperation {
    Setup { apply: bool },
    Status,
    Repair { apply: bool },
    Uninstall { apply: bool },
}

/// Operator answer to the single host-permission consent step (bot#1239).
///
/// `Ask` prompts once on an interactive TTY; non-interactive sessions without
/// an explicit answer skip the grant and report it honestly as `missing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HostPermissionConsent {
    /// Authorize Bicameral to configure the Bicameral-server-scoped grant.
    Consent,
    /// Leave every Bicameral MCP tool host-prompted.
    Skip,
    /// Prompt on an interactive TTY; otherwise behave like `Skip`.
    Ask,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PublicReleaseManifest {
    version: String,
    mcp: Option<McpReleaseBinding>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct McpReleaseBinding {
    package: String,
    version: String,
    wheel_url: String,
    wheel_sha256: String,
    release_descriptor_digest: String,
    tool_request_protocol: String,
    python_requires: String,
}

#[derive(Debug)]
struct SetupPaths {
    tool_dir: PathBuf,
    bin_dir: PathBuf,
    executable: PathBuf,
    binding_file: PathBuf,
    permissions: HostPermissionPaths,
}

impl SetupPaths {
    fn resolve() -> anyhow::Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is unavailable; refusing operator-local MCP setup")?;
        let share = home.join(".local/share/bicameral");
        let bin_dir = share.join("bin");
        Ok(Self {
            tool_dir: share.join("mcp-tools"),
            executable: bin_dir.join("bicameral-mcp"),
            binding_file: share.join("mcp-setup-binding-v1.json"),
            permissions: HostPermissionPaths::resolve(&home, &share),
            bin_dir,
        })
    }

    fn create(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.tool_dir)?;
        fs::create_dir_all(&self.bin_dir)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SetupBindingEnvelope {
    schema_version: u8,
    bot_version: String,
    manifest_version: String,
    mcp: McpReleaseBinding,
}

#[derive(Debug)]
struct ProcessCapture {
    success: bool,
    stdout: String,
}

#[derive(Debug, Default)]
struct HostInspection {
    package_installed: bool,
    registration_verified: bool,
    managed_hook_verified: bool,
    drift_reason_codes: Vec<McpDriftCode>,
    activation: Option<VerifiedActivationEvidence>,
    host_permission: Option<HostPermissionObservation>,
}

#[derive(Debug, Deserialize)]
struct AdapterStatus {
    state: String,
    package_matches: bool,
    artifact_generation: String,
    activation_receipt_dir: String,
}

#[derive(Debug, Deserialize)]
struct LocalActivationProjection {
    state: String,
    verified: bool,
    artifact_generation: String,
    correlation_id: Option<String>,
    receipt_digest: String,
    verified_at: Option<String>,
    expires_at: Option<String>,
}

pub async fn run(
    hosts: Vec<DeveloperHost>,
    operation: DeveloperHostOperation,
    workspace: Option<&Path>,
    permission_consent: HostPermissionConsent,
) -> anyhow::Result<()> {
    let paths = SetupPaths::resolve()?;
    let manifest = load_operation_manifest(operation, &paths).await?;
    let binding = validate_binding(&manifest)?;
    let base_url = gateway_base_url(workspace)?;
    let client = reqwest::Client::builder()
        .user_agent("bicameral-setup-controller")
        .build()?;
    let selected = resolve_hosts(hosts, operation)?;

    println!(
        "Bicameral v{} MCP binding: {} v{} ({})",
        manifest.version, binding.package, binding.version, binding.release_descriptor_digest
    );
    println!("Managed executable: {}", paths.executable.display());
    println!(
        "Hosts: {}",
        selected
            .iter()
            .map(|h| h.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    if matches!(operation, DeveloperHostOperation::Uninstall { apply: true }) {
        for host in &selected {
            print_plan(*host, operation, binding, &paths, &[]);
            // Remove only the Bicameral-owned permission grant first, while
            // the host registration it belongs to still exists. A malformed
            // ownership record fails closed: the host configuration and the
            // record are both left untouched for inspection.
            match host_permissions::remove(*host, &paths.permissions) {
                Ok(removal) => {
                    for rule in &removal.removed {
                        println!("  removed host permission: {rule}");
                    }
                    for rule in &removal.preserved_operator_changes {
                        println!(
                            "  preserved operator-modified host permission (ownership \
                             retained, not clobbered): {rule}"
                        );
                    }
                }
                Err(error) => println!(
                    "  host permission: ownership record could not be read ({error}); host \
                     configuration left unchanged and the record preserved for inspection"
                ),
            }
            uninstall_host_surfaces(*host, &paths)?;
        }
        uninstall_package(&paths)?;
        for host in &selected {
            let readiness = submit_readiness(
                &client,
                &base_url,
                &observation(*host, binding, HostInspection::default(), Vec::new()),
            )
            .await?;
            println!(
                "{} readiness: {}",
                host.as_str(),
                readiness["state"].as_str().unwrap_or("unknown")
            );
        }
        if paths.binding_file.is_file() {
            fs::remove_file(&paths.binding_file)?;
        }
        return Ok(());
    }

    // One explicit consent decision per invocation covers every selected
    // host (bot#1239 item 1). Hosts with a recorded grant carry their earlier
    // consent forward, so status/repair never re-prompt.
    let invocation_consent = match operation {
        DeveloperHostOperation::Setup { apply: true }
        | DeveloperHostOperation::Repair { apply: true } => {
            resolve_permission_consent(&selected, &paths, permission_consent)?
        }
        _ => false,
    };

    for host in selected {
        operate_host(
            &client,
            &base_url,
            host,
            operation,
            &manifest.version,
            binding,
            &paths,
            invocation_consent,
        )
        .await
        .with_context(|| format!("{} MCP setup failed", host.as_str()))?;
    }
    Ok(())
}

/// Resolve the single explicit consent step for the Bicameral host grant,
/// once per setup invocation for every selected host.
///
/// Hosts with a previously recorded grant carry that consent forward and are
/// not asked again; `Ask` prompts once on an interactive TTY, naming only the
/// hosts that still need consent. A malformed ownership record does not
/// count as recorded consent; the planner fails closed on it separately.
fn resolve_permission_consent(
    hosts: &[DeveloperHost],
    paths: &SetupPaths,
    requested: HostPermissionConsent,
) -> anyhow::Result<bool> {
    let needing_consent: Vec<DeveloperHost> = hosts
        .iter()
        .copied()
        .filter(|host| {
            !matches!(
                host_permissions::owned_grant(&paths.permissions, *host),
                Ok(Some(_))
            )
        })
        .collect();
    if needing_consent.is_empty() {
        return Ok(true);
    }
    match requested {
        HostPermissionConsent::Consent => Ok(true),
        HostPermissionConsent::Skip => Ok(false),
        HostPermissionConsent::Ask => {
            if !interactive_tty() {
                println!(
                    "  host permission: not configured (non-interactive; pass \
                     --host-permissions consent to authorize the Bicameral-scoped grant)"
                );
                return Ok(false);
            }
            let names = needing_consent
                .iter()
                .map(|host| host_display_name(*host))
                .collect::<Vec<_>>()
                .join(" and ");
            let consented = cliclack::confirm(format!(
                "Allow Bicameral to configure {names} permissions for the Bicameral MCP server \
                 only? This adds one server-scoped grant per host and never changes shell, \
                 sandbox, network, or other MCP permissions."
            ))
            .initial_value(true)
            .interact()?;
            Ok(consented)
        }
    }
}

fn host_display_name(host: DeveloperHost) -> &'static str {
    match host {
        DeveloperHost::Claude => "Claude Code",
        DeveloperHost::Codex => "Codex",
    }
}

/// Declared tool registry from the exact installed MCP generation. Used only
/// for the registry fallback; a text-only listing carries no action classes.
fn declared_tool_registry(paths: &SetupPaths) -> Option<DeclaredToolRegistry> {
    if !paths.executable.is_file() {
        return None;
    }
    let listing = capture_path(&paths.executable, &["tools", "--json"])
        .ok()
        .filter(|result| result.success)
        .or_else(|| {
            capture_path(&paths.executable, &["tools"])
                .ok()
                .filter(|result| result.success)
        })?;
    Some(DeclaredToolRegistry::parse(&listing.stdout))
}

fn reconcile_host_permission(
    host: DeveloperHost,
    paths: &SetupPaths,
    consent: bool,
) -> anyhow::Result<()> {
    let capabilities = HostPermissionCapabilities::for_host(host);
    let registry = if capabilities.server_scoped {
        None
    } else {
        declared_tool_registry(paths)
    };
    let outcome = host_permissions::reconcile(
        host,
        &paths.permissions,
        capabilities,
        consent,
        registry.as_ref(),
        &Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )?;
    let verb = if outcome.applied {
        "applied"
    } else if consent {
        "no change"
    } else {
        "not applied (no consent)"
    };
    println!(
        "  {} [{verb}] -> {}",
        host_permissions::describe_plan(host, &outcome.plan),
        outcome.observation.state.as_str()
    );
    Ok(())
}

/// Whether default `setup mcp` should mutate after printing the plan.
///
/// Apply is the default; only an explicit `--dry-run` suppresses install.
pub fn setup_should_apply(dry_run: bool) -> bool {
    !dry_run
}

/// Resolve coding hosts for setup.
///
/// - Explicit `--host` flags always win (scriptable).
/// - Interactive setup (no flags) → cliclack multi-select (Clack-style picker).
/// - Non-interactive setup without flags → fail closed (require `--host`).
/// - status / repair / uninstall without flags → both hosts (inspect/repair all).
fn resolve_hosts(
    hosts: Vec<DeveloperHost>,
    operation: DeveloperHostOperation,
) -> anyhow::Result<Vec<DeveloperHost>> {
    if !hosts.is_empty() {
        return Ok(dedupe_hosts(hosts));
    }

    let is_setup = matches!(operation, DeveloperHostOperation::Setup { .. });
    if is_setup {
        if interactive_tty() {
            return interactive_host_pick();
        }
        bail!(
            "no coding host selected: pass --host claude and/or --host codex \
             (non-interactive sessions cannot open the host picker)"
        );
    }

    // Status/repair/uninstall: inspect or mutate every supported host.
    Ok(vec![DeveloperHost::Claude, DeveloperHost::Codex])
}

fn interactive_tty() -> bool {
    if std::env::var_os("CI").is_some() {
        return false;
    }
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

fn interactive_host_pick() -> anyhow::Result<Vec<DeveloperHost>> {
    let claude_hint = if host_available(DeveloperHost::Claude) {
        "detected on PATH"
    } else {
        "not found on PATH"
    };
    let codex_hint = if host_available(DeveloperHost::Codex) {
        "detected on PATH"
    } else {
        "not found on PATH"
    };

    cliclack::intro("Bicameral MCP setup")?;
    let mut initial = Vec::new();
    if host_available(DeveloperHost::Claude) {
        initial.push("claude");
    }
    if host_available(DeveloperHost::Codex) {
        initial.push("codex");
    }
    // Prefer at least one pre-selected option so multiselect is not empty-first.
    if initial.is_empty() {
        initial.push("claude");
    }

    let selected: Vec<&str> =
        cliclack::multiselect("Which coding hosts should install Bicameral MCP?")
            .required(true)
            .initial_values(initial)
            .item("claude", "Claude Code", claude_hint)
            .item("codex", "Codex CLI", codex_hint)
            .interact()?;

    let hosts = selected
        .into_iter()
        .filter_map(|id| match id {
            "claude" => Some(DeveloperHost::Claude),
            "codex" => Some(DeveloperHost::Codex),
            _ => None,
        })
        .collect::<Vec<_>>();
    if hosts.is_empty() {
        bail!("select at least one coding host, or pass --host explicitly");
    }
    Ok(hosts)
}

fn host_available(host: DeveloperHost) -> bool {
    Command::new(host.program())
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn dedupe_hosts(hosts: Vec<DeveloperHost>) -> Vec<DeveloperHost> {
    let mut out = Vec::new();
    for host in hosts {
        if !out.contains(&host) {
            out.push(host);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
async fn operate_host(
    client: &reqwest::Client,
    base_url: &str,
    host: DeveloperHost,
    operation: DeveloperHostOperation,
    manifest_version: &str,
    binding: &McpReleaseBinding,
    paths: &SetupPaths,
    invocation_consent: bool,
) -> anyhow::Result<()> {
    let (missing, host_version) = preflight(client, base_url, host, binding).await;
    print_plan(host, operation, binding, paths, &missing);

    match operation {
        // Plan-only: setup with --dry-run, or repair/uninstall without --yes.
        DeveloperHostOperation::Setup { apply: false }
        | DeveloperHostOperation::Repair { apply: false }
        | DeveloperHostOperation::Uninstall { apply: false } => return Ok(()),
        DeveloperHostOperation::Status => {}
        // Default setup path applies after print_plan (apply: true unless --dry-run).
        DeveloperHostOperation::Setup { apply: true }
        | DeveloperHostOperation::Repair { apply: true } => {
            if !missing.is_empty() {
                let observation = observation(host, binding, HostInspection::default(), missing);
                let _ = submit_readiness(client, base_url, &observation).await;
                bail!("required setup prerequisites are unavailable");
            }
            paths.create()?;
            install_exact_package(client, binding, paths).await?;
            persist_binding(manifest_version, binding, paths)?;
            ensure_registration(host, &paths.executable, true)?;
            install_adapter(host, binding, host_version.as_deref().unwrap_or(""), paths)?;
            // bot#1239: one explicit consent step, then the minimum scoped
            // Bicameral-server grant. Re-running on correct state is a no-op.
            let consent = invocation_consent
                || matches!(
                    host_permissions::owned_grant(&paths.permissions, host),
                    Ok(Some(_))
                );
            reconcile_host_permission(host, paths, consent)?;
        }
        DeveloperHostOperation::Uninstall { apply: true } => unreachable!(),
    }

    let inspection = inspect_host(host, binding, paths)?;
    let readiness = submit_readiness(
        client,
        base_url,
        &observation(host, binding, inspection, missing),
    )
    .await?;
    println!(
        "{} readiness: {}",
        host.as_str(),
        readiness["state"].as_str().unwrap_or("unknown")
    );
    Ok(())
}

fn validate_binding(manifest: &PublicReleaseManifest) -> anyhow::Result<&McpReleaseBinding> {
    let binding = manifest
        .mcp
        .as_ref()
        .context("release_binding_missing: manifest has no MCP binding")?;
    if binding.package != "bicameral-mcp" {
        bail!("release binding package must be bicameral-mcp");
    }
    if binding.version.trim().is_empty() {
        bail!("release binding MCP version is empty");
    }
    let wheel = reqwest::Url::parse(&binding.wheel_url)?;
    if wheel.scheme() != "https" {
        bail!("release binding wheel URL must use HTTPS");
    }
    require_hex_digest(&binding.wheel_sha256, false)?;
    require_hex_digest(&binding.release_descriptor_digest, true)?;
    if binding.tool_request_protocol != "v2" {
        bail!("release binding ToolRequest protocol must be v2");
    }
    if !binding.python_requires.trim().starts_with(">=3.11") {
        bail!("release binding Python requirement must require Python 3.11 or newer");
    }
    Ok(binding)
}

fn require_hex_digest(value: &str, prefixed: bool) -> anyhow::Result<()> {
    let value = if prefixed {
        value
            .strip_prefix("sha256:")
            .context("release binding descriptor digest must use sha256:")?
    } else {
        value
    };
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("release binding digest must contain 64 lowercase hexadecimal characters");
    }
    Ok(())
}

async fn load_manifest() -> anyhow::Result<PublicReleaseManifest> {
    if let Some(path) = std::env::var_os(MANIFEST_PATH_ENV) {
        let bytes = fs::read(PathBuf::from(path))?;
        return serde_json::from_slice(&bytes).context("release manifest is invalid JSON");
    }
    let base = std::env::var(DOWNLOAD_BASE_ENV)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DOWNLOAD_BASE_URL.to_string());
    let response = reqwest::Client::builder()
        .user_agent("bicameral-setup-controller")
        .build()?
        .get(format!("{base}/latest.json"))
        .send()
        .await?
        .error_for_status()?;
    let manifest: PublicReleaseManifest = response
        .json()
        .await
        .context("public release manifest is invalid")?;
    let running = env!("CARGO_PKG_VERSION");
    if manifest.version.trim_start_matches('v') != running {
        bail!(
            "public release manifest v{} does not bind the running Bot v{}",
            manifest.version,
            running
        );
    }
    Ok(manifest)
}

async fn load_operation_manifest(
    operation: DeveloperHostOperation,
    paths: &SetupPaths,
) -> anyhow::Result<PublicReleaseManifest> {
    if !matches!(operation, DeveloperHostOperation::Setup { .. }) && paths.binding_file.is_file() {
        return load_persisted_binding(paths);
    }
    load_manifest().await
}

fn persist_binding(
    manifest_version: &str,
    binding: &McpReleaseBinding,
    paths: &SetupPaths,
) -> anyhow::Result<()> {
    require_hex_digest(&binding.wheel_sha256, false)?;
    require_hex_digest(&binding.release_descriptor_digest, true)?;
    let envelope = SetupBindingEnvelope {
        schema_version: 1,
        bot_version: env!("CARGO_PKG_VERSION").into(),
        manifest_version: manifest_version.into(),
        mcp: binding.clone(),
    };
    let parent = paths
        .binding_file
        .parent()
        .context("setup binding path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".mcp-setup-binding-v1.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&envelope)?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    fs::write(&temporary, &bytes)?;
    fs::rename(temporary, &paths.binding_file)?;
    Ok(())
}

fn load_persisted_binding(paths: &SetupPaths) -> anyhow::Result<PublicReleaseManifest> {
    let envelope: SetupBindingEnvelope = serde_json::from_slice(&fs::read(&paths.binding_file)?)?;
    if envelope.schema_version != 1 {
        bail!(
            "unsupported MCP setup binding schema {}",
            envelope.schema_version
        );
    }
    let manifest = PublicReleaseManifest {
        version: envelope.manifest_version,
        mcp: Some(envelope.mcp),
    };
    validate_binding(&manifest)?;
    Ok(manifest)
}

async fn preflight(
    client: &reqwest::Client,
    base_url: &str,
    host: DeveloperHost,
    _binding: &McpReleaseBinding,
) -> (Vec<McpPrerequisiteCode>, Option<String>) {
    let mut missing = Vec::new();
    if !capture("uv", &["--version"]).is_ok_and(|version| version.success) {
        missing.push(McpPrerequisiteCode::UvMissing);
    }
    match capture("python3", &["--version"]) {
        Ok(version) if version.success && python_is_supported(&version.stdout) => {}
        _ => missing.push(McpPrerequisiteCode::PythonUnsupported),
    }
    let host_version = match capture(host.program(), &["--version"]) {
        Ok(version) if version.success && !version.stdout.is_empty() => Some(version.stdout),
        _ => {
            missing.push(McpPrerequisiteCode::DeveloperHostMissing);
            None
        }
    };
    let daemon_ok = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success());
    if !daemon_ok {
        missing.push(McpPrerequisiteCode::DaemonUnavailable);
    }
    (missing, host_version)
}

fn python_is_supported(output: &str) -> bool {
    let version = output
        .split_whitespace()
        .find(|part| part.chars().next().is_some_and(|c| c.is_ascii_digit()));
    let Some(version) = version else {
        return false;
    };
    let mut parts = version.split('.');
    let major = parts.next().and_then(|value| value.parse::<u32>().ok());
    let minor = parts.next().and_then(|value| value.parse::<u32>().ok());
    matches!((major, minor), (Some(major), Some(minor)) if major > 3 || (major == 3 && minor >= 11))
}

fn print_plan(
    host: DeveloperHost,
    operation: DeveloperHostOperation,
    binding: &McpReleaseBinding,
    paths: &SetupPaths,
    missing: &[McpPrerequisiteCode],
) {
    println!();
    println!("{}:", host.as_str());
    println!("  operation: {operation:?}");
    println!("  wheel: {}", binding.wheel_url);
    println!("  wheel sha256: {}", binding.wheel_sha256);
    println!("  tool root: {}", paths.tool_dir.display());
    println!("  bin root: {}", paths.bin_dir.display());
    println!("  host registration: user-scoped bicameral stdio server");
    println!("  managed hook: package adapter only");
    let capabilities = HostPermissionCapabilities::for_host(host);
    let plan = host_permissions::plan(host, &paths.permissions, capabilities, None).unwrap_or_else(
        |error| PermissionPlan::Malformed {
            reason: error.to_string(),
        },
    );
    println!("  {}", host_permissions::describe_plan(host, &plan));
    println!(
        "  rollback: Bicameral-owned host permission, managed hook, registration, then MCP tool \
         environment"
    );
    if !missing.is_empty() {
        println!("  missing prerequisites: {missing:?}");
    }
}

async fn install_exact_package(
    client: &reqwest::Client,
    binding: &McpReleaseBinding,
    paths: &SetupPaths,
) -> anyhow::Result<()> {
    let response = client
        .get(&binding.wheel_url)
        .send()
        .await?
        .error_for_status()?;
    let wheel_bytes = response.bytes().await?;
    let actual = hex_sha256(&wheel_bytes);
    if actual != binding.wheel_sha256 {
        bail!(
            "MCP wheel checksum mismatch: expected {}, got {}",
            binding.wheel_sha256,
            actual
        );
    }
    let temporary = tempfile::tempdir()?;
    let wheel = temporary.path().join(format!(
        "bicameral_mcp-{}-py3-none-any.whl",
        binding.version
    ));
    fs::write(&wheel, &wheel_bytes)?;
    run_uv_tool(
        paths,
        &[
            "tool".into(),
            "install".into(),
            "--force".into(),
            wheel.display().to_string(),
        ],
    )?;
    if !paths.executable.is_file() {
        bail!("uv completed without materializing the managed bicameral-mcp executable");
    }
    let version = capture_path(&paths.executable, &["--version"])?;
    if !version.success || !version.stdout.contains(&binding.version) {
        bail!("installed bicameral-mcp version does not match the release binding");
    }
    Ok(())
}

fn ensure_registration(
    host: DeveloperHost,
    executable: &Path,
    reconcile: bool,
) -> anyhow::Result<()> {
    if registration_matches(host, executable) {
        return Ok(());
    }
    if reconcile {
        let _ = run_host_remove(host);
    }
    let executable = executable.display().to_string();
    let args = registration_add_args(host, &executable);
    run_checked(host.program(), &args)?;
    if !registration_matches(host, Path::new(&executable)) {
        bail!("host registration did not verify the exact managed executable");
    }
    Ok(())
}

fn registration_add_args(host: DeveloperHost, executable: &str) -> Vec<String> {
    match host {
        DeveloperHost::Claude => vec![
            "mcp".into(),
            "add".into(),
            "--scope".into(),
            "user".into(),
            "--transport".into(),
            "stdio".into(),
            "bicameral".into(),
            "--".into(),
            executable.into(),
        ],
        DeveloperHost::Codex => vec![
            "mcp".into(),
            "add".into(),
            "bicameral".into(),
            "--".into(),
            executable.into(),
        ],
    }
}

fn registration_matches(host: DeveloperHost, executable: &Path) -> bool {
    let args: &[&str] = match host {
        DeveloperHost::Claude => &["mcp", "get", "bicameral"],
        DeveloperHost::Codex => &["mcp", "list"],
    };
    capture(host.program(), args).is_ok_and(|result| {
        result.success
            && result.stdout.contains("bicameral")
            && result.stdout.contains(&executable.display().to_string())
    })
}

fn run_host_remove(host: DeveloperHost) -> anyhow::Result<()> {
    run_checked(
        host.program(),
        &["mcp".into(), "remove".into(), "bicameral".into()],
    )
}

fn install_adapter(
    host: DeveloperHost,
    binding: &McpReleaseBinding,
    host_version: &str,
    paths: &SetupPaths,
) -> anyhow::Result<()> {
    run_checked_path(
        &paths.executable,
        &[
            "adapters".into(),
            "install".into(),
            "--host".into(),
            host.as_str().into(),
            "--consent".into(),
            "--artifact-generation".into(),
            binding.release_descriptor_digest.clone(),
            "--host-version".into(),
            host_version.into(),
            "--json".into(),
        ],
    )
}

fn inspect_host(
    host: DeveloperHost,
    binding: &McpReleaseBinding,
    paths: &SetupPaths,
) -> anyhow::Result<HostInspection> {
    let mut result = HostInspection::default();
    if !paths.executable.is_file() {
        return Ok(result);
    }
    let package = capture_path(&paths.executable, &["--version"])?;
    result.package_installed = package.success && package.stdout.contains(&binding.version);
    if !result.package_installed {
        result.drift_reason_codes.push(McpDriftCode::PackageDrift);
    }
    result.registration_verified = registration_matches(host, &paths.executable);
    if !result.registration_verified {
        result
            .drift_reason_codes
            .push(McpDriftCode::HostRegistrationDrift);
    }
    result.host_permission = Some(host_permissions::observe(
        host,
        &paths.permissions,
        HostPermissionCapabilities::for_host(host),
    ));
    let statuses: Vec<AdapterStatus> = run_json_path(
        &paths.executable,
        &["adapters", "status", "--host", host.as_str(), "--json"],
    )?;
    let status = statuses
        .into_iter()
        .next()
        .context("packaged adapter returned no host status")?;
    result.managed_hook_verified = status.state == "installed_enabled"
        && status.package_matches
        && status.artifact_generation == binding.release_descriptor_digest;
    if !result.managed_hook_verified {
        result
            .drift_reason_codes
            .push(McpDriftCode::ManagedHookDrift);
        return Ok(result);
    }
    if let Some(receipt) = newest_receipt(Path::new(&status.activation_receipt_dir))? {
        // verify-host exits non-zero for schema/topology rejection and for
        // honest authentic:false. Installation/setup must still succeed; only
        // VerifiedActivationEvidence requires authentic acceptance.
        let capture = capture_path(
            &paths.executable,
            &[
                "adapters",
                "verify-host",
                "--host",
                host.as_str(),
                "--receipt",
                &receipt.display().to_string(),
                "--artifact-generation",
                &binding.release_descriptor_digest,
                "--json",
            ],
        )?;
        if let Ok(verified) = serde_json::from_str::<LocalActivationProjection>(&capture.stdout) {
            if verified.verified
                && verified.state == "active_verified"
                && verified.artifact_generation == binding.release_descriptor_digest
            {
                if let (Some(correlation_id), Some(verified_at), Some(expires_at)) = (
                    verified.correlation_id.clone(),
                    verified.verified_at.clone(),
                    verified.expires_at.clone(),
                ) {
                    result.activation = Some(VerifiedActivationEvidence {
                        correlation_id,
                        receipt_digest: verified.receipt_digest,
                        artifact_generation: verified.artifact_generation,
                        verified_at,
                        expires_at,
                    });
                }
            }
        }
    }
    Ok(result)
}

fn newest_receipt(directory: &Path) -> anyhow::Result<Option<PathBuf>> {
    if !directory.is_dir() {
        return Ok(None);
    }
    let mut receipts = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    receipts.sort_by_key(|path| fs::metadata(path).and_then(|value| value.modified()).ok());
    Ok(receipts.pop())
}

fn observation(
    host: DeveloperHost,
    binding: &McpReleaseBinding,
    inspection: HostInspection,
    missing_prerequisites: Vec<McpPrerequisiteCode>,
) -> McpReadinessObservation {
    McpReadinessObservation {
        subject_id: host.id(),
        artifact_generation: binding.release_descriptor_digest.clone(),
        package_installed: inspection.package_installed,
        registration_verified: inspection.registration_verified,
        managed_hook_verified: inspection.managed_hook_verified,
        missing_prerequisites,
        drift_reason_codes: inspection.drift_reason_codes,
        activation: inspection.activation,
        host_permission: inspection.host_permission,
        observed_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }
}

async fn submit_readiness(
    client: &reqwest::Client,
    base_url: &str,
    observation: &McpReadinessObservation,
) -> anyhow::Result<serde_json::Value> {
    let response = client
        .post(format!("{base_url}/api/v1/setup/capability-readiness"))
        .json(observation)
        .send()
        .await
        .context("local Bicameral daemon is not reachable")?;
    let status = response.status();
    let body = response.bytes().await?;
    if !status.is_success() {
        bail!("local Bicameral daemon rejected setup evidence with HTTP {status}");
    }
    serde_json::from_slice(&body).context("daemon returned invalid readiness JSON")
}

fn uninstall_host_surfaces(host: DeveloperHost, paths: &SetupPaths) -> anyhow::Result<()> {
    if paths.executable.is_file() {
        run_checked_path(
            &paths.executable,
            &[
                "adapters".into(),
                "uninstall".into(),
                "--host".into(),
                host.as_str().into(),
                "--json".into(),
            ],
        )?;
    }
    if registration_matches(host, &paths.executable) {
        run_host_remove(host)?;
    }
    Ok(())
}

fn uninstall_package(paths: &SetupPaths) -> anyhow::Result<()> {
    if paths.tool_dir.is_dir() {
        run_uv_tool(
            paths,
            &["tool".into(), "uninstall".into(), "bicameral-mcp".into()],
        )?;
    }
    Ok(())
}

fn gateway_base_url(workspace: Option<&Path>) -> anyhow::Result<String> {
    let mut config = match workspace {
        Some(start) => WorkspaceDiscovery::discover(start)?.1,
        None => BicameralConfig::default(),
    };
    if let Ok(port) = std::env::var(GATEWAY_PORT_ENV) {
        config.gateway.port = port
            .parse::<u16>()
            .context("BICAMERAL_GATEWAY_PORT must be a valid u16")?;
    }
    let host = if config.gateway.host == "::1" {
        "[::1]".to_string()
    } else {
        config.gateway.host
    };
    Ok(format!("http://{}:{}", host, config.gateway.port))
}

fn capture(program: &str, args: &[&str]) -> anyhow::Result<ProcessCapture> {
    let output = Command::new(program).args(args).output()?;
    let stdout = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    Ok(ProcessCapture {
        success: output.status.success(),
        stdout: sanitized_output(stdout),
    })
}

fn capture_path(program: &Path, args: &[&str]) -> anyhow::Result<ProcessCapture> {
    let output = Command::new(program).args(args).output()?;
    let stdout = if output.stdout.is_empty() {
        &output.stderr
    } else {
        &output.stdout
    };
    Ok(ProcessCapture {
        success: output.status.success(),
        stdout: sanitized_output(stdout),
    })
}

fn sanitized_output(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|character| !character.is_control() || *character == '\n')
        .take(4096)
        .collect::<String>()
        .trim()
        .to_string()
}

fn run_checked(program: &str, args: &[String]) -> anyhow::Result<()> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        bail!(
            "{program} exited with status {}",
            output.status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn run_uv_tool(paths: &SetupPaths, args: &[String]) -> anyhow::Result<()> {
    let output = Command::new("uv")
        .env("UV_TOOL_DIR", &paths.tool_dir)
        .env("UV_TOOL_BIN_DIR", &paths.bin_dir)
        .args(args)
        .output()?;
    if !output.status.success() {
        bail!(
            "uv exited with status {}",
            output.status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn run_checked_path(program: &Path, args: &[String]) -> anyhow::Result<()> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        bail!(
            "{} exited with status {}",
            program.display(),
            output.status.code().unwrap_or(-1)
        );
    }
    Ok(())
}

fn run_json_path<T: serde::de::DeserializeOwned>(
    program: &Path,
    args: &[&str],
) -> anyhow::Result<T> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        bail!(
            "{} exited with status {}",
            program.display(),
            output.status.code().unwrap_or(-1)
        );
    }
    serde_json::from_slice(&output.stdout).context("packaged MCP command returned invalid JSON")
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> PublicReleaseManifest {
        PublicReleaseManifest {
            version: "0.1.14".into(),
            mcp: Some(McpReleaseBinding {
                package: "bicameral-mcp".into(),
                version: "0.17.0".into(),
                wheel_url: "https://example.test/bicameral_mcp-0.17.0-py3-none-any.whl".into(),
                wheel_sha256: "a".repeat(64),
                release_descriptor_digest: format!("sha256:{}", "b".repeat(64)),
                tool_request_protocol: "v2".into(),
                python_requires: ">=3.11".into(),
            }),
        }
    }

    #[test]
    fn release_manifest_requires_the_exact_mcp_compatibility_binding() {
        assert!(validate_binding(&manifest()).is_ok());

        let mut missing = manifest();
        missing.mcp = None;
        assert!(validate_binding(&missing)
            .unwrap_err()
            .to_string()
            .contains("release_binding_missing"));

        let mut floating = manifest();
        floating.mcp.as_mut().unwrap().wheel_sha256 = "latest".into();
        assert!(validate_binding(&floating).is_err());
    }

    #[test]
    fn registration_commands_are_fixed_argument_vectors_without_a_shell() {
        let claude = registration_add_args(DeveloperHost::Claude, "/managed/bicameral-mcp");
        assert_eq!(
            claude,
            [
                "mcp",
                "add",
                "--scope",
                "user",
                "--transport",
                "stdio",
                "bicameral",
                "--",
                "/managed/bicameral-mcp"
            ]
        );
        let codex = registration_add_args(DeveloperHost::Codex, "/managed/bicameral-mcp");
        assert_eq!(
            codex,
            ["mcp", "add", "bicameral", "--", "/managed/bicameral-mcp"]
        );
    }

    #[test]
    fn python_floor_check_rejects_old_or_unparseable_runtimes() {
        assert!(python_is_supported("Python 3.11.9"));
        assert!(python_is_supported("Python 3.14.0"));
        assert!(!python_is_supported("Python 3.10.14"));
        assert!(!python_is_supported("unknown"));
    }

    #[test]
    fn setup_apply_defaults_true_unless_dry_run() {
        // CLI: `bicameral setup mcp` → apply true; `--dry-run` → apply false.
        assert!(setup_should_apply(/* dry_run */ false));
        assert!(!setup_should_apply(/* dry_run */ true));
    }

    #[test]
    fn resolve_hosts_prefers_explicit_flags() {
        let hosts = resolve_hosts(
            vec![
                DeveloperHost::Codex,
                DeveloperHost::Claude,
                DeveloperHost::Codex,
            ],
            DeveloperHostOperation::Setup { apply: false },
        )
        .unwrap();
        assert_eq!(hosts, vec![DeveloperHost::Codex, DeveloperHost::Claude]);
    }

    #[test]
    fn resolve_hosts_status_defaults_to_both_without_flags() {
        // Force non-interactive path regardless of local TTY.
        std::env::set_var("CI", "1");
        let hosts = resolve_hosts(vec![], DeveloperHostOperation::Status).unwrap();
        std::env::remove_var("CI");
        assert_eq!(hosts, vec![DeveloperHost::Claude, DeveloperHost::Codex]);
    }

    #[test]
    fn resolve_hosts_noninteractive_setup_requires_host_flag() {
        std::env::set_var("CI", "1");
        let err = resolve_hosts(vec![], DeveloperHostOperation::Setup { apply: true })
            .unwrap_err()
            .to_string();
        std::env::remove_var("CI");
        assert!(
            err.contains("--host"),
            "expected --host guidance, got: {err}"
        );
    }

    #[test]
    fn persisted_binding_keeps_status_repair_and_uninstall_off_latest() {
        let root = tempfile::tempdir().unwrap();
        let paths = SetupPaths {
            tool_dir: root.path().join("tools"),
            bin_dir: root.path().join("bin"),
            executable: root.path().join("bin/bicameral-mcp"),
            binding_file: root.path().join("mcp-setup-binding-v1.json"),
            permissions: HostPermissionPaths::resolve(root.path(), root.path()),
        };
        let manifest = manifest();
        let binding = validate_binding(&manifest).unwrap();

        persist_binding(&manifest.version, binding, &paths).unwrap();
        let loaded = load_persisted_binding(&paths).unwrap();

        assert_eq!(loaded.version, "0.1.14");
        let loaded_binding = validate_binding(&loaded).unwrap();
        assert_eq!(
            loaded_binding.release_descriptor_digest,
            binding.release_descriptor_digest
        );
    }
}
