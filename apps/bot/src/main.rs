//! Bicameral CLI — local governance runtime for software teams.
//!
//! Commands: init, onboard, ingest, preflight, review, mod validate, mod run,
//! gateway start, pairing pending/approve/deny,
//! service install/start/stop/status/uninstall, doctor,
//! update, uninstall.

use clap::{Parser, Subcommand};
use commands::setup::{DeveloperHost, DeveloperHostOperation, HostPermissionConsent};
use std::path::PathBuf;

mod commands;

#[derive(Parser)]
#[command(name = "bicameral")]
#[command(about = "Local-first governance runtime for software teams.\n\n\
    The Decision Ledger contract is substrate-neutral: both Git and Drive/basic-store\n\
    substrates satisfy the baseline ledger purpose (append, replay, audit). Git extends\n\
    the baseline with PR-resolution capabilities when configured.")]
#[command(version)]
struct Cli {
    /// Workspace root (defaults to current directory discovery)
    #[arg(long, global = true)]
    workspace: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Bicameral workspace
    Init {
        /// Path to initialize (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Ingest a source into the governance pipeline (substrate-neutral)
    Ingest {
        /// Path to a JSON file containing the source evidence
        file: PathBuf,
    },
    /// Run preflight checks against current decisions (substrate-neutral)
    Preflight {
        /// Path or ref to check
        #[arg(default_value = ".")]
        target: String,
    },
    /// Submit or manage review commands (substrate-neutral)
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
    /// Mod manifest operations
    Mod {
        #[command(subcommand)]
        action: ModAction,
    },
    /// Gateway operations (serves API for any configured substrate)
    Gateway {
        #[command(subcommand)]
        action: GatewayAction,
    },
    /// Confirm hosted-browser pairing requests through the local daemon
    Pairing {
        #[command(subcommand)]
        action: PairingAction,
    },
    /// Onboard: first-time setup and daemon installation
    Onboard {
        /// Install the daemon as a user-level service
        #[arg(long)]
        install_daemon: bool,
    },
    /// Deprecated alias: start the daemon API (the SPA is Cloud-hosted)
    Dashboard,
    /// Service lifecycle management
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Configure operator-local runtime capabilities
    Setup {
        #[command(subcommand)]
        target: SetupTarget,
    },
    /// Run diagnostic checks (shows substrate kind, capabilities, and configuration)
    Doctor,
    /// Check for and install updates from GitHub releases
    Update {
        /// Only check for updates without installing
        #[arg(long)]
        check: bool,
    },
    /// Remove Bicameral binary, daemon service, and optionally all data
    Uninstall {
        /// Remove everything including ~/.bicameral/ decision data and user config
        #[arg(long)]
        all: bool,
    },
}

#[derive(Subcommand)]
enum ReviewAction {
    /// List pending reviews
    List,
    /// Submit a candidate for review
    Submit {
        /// Candidate ID
        id: String,
    },
}

#[derive(Subcommand)]
enum ModAction {
    /// Validate a mod manifest
    Validate {
        /// Path to mod manifest YAML
        manifest: PathBuf,
    },
    /// Run a mod against a fixture
    Run {
        /// Path to mod manifest YAML
        manifest: PathBuf,
        /// Path to fixture JSON file
        #[arg(long)]
        fixture: PathBuf,
    },
}

#[derive(Subcommand)]
enum GatewayAction {
    /// Start the local gateway
    Start,
}

#[derive(Subcommand)]
enum PairingAction {
    /// List safe summaries of pending hosted pairing requests
    Pending,
    /// Match a browser code, verify its origin/account, then explicitly approve
    Approve,
    /// Match a browser code, verify its origin/account, then explicitly deny
    Deny,
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Install the daemon as a user-level service
    Install,
    /// Start the daemon service
    Start,
    /// Stop the daemon service
    Stop,
    /// Show daemon service status
    Status,
    /// Remove the daemon service configuration
    Uninstall,
}

#[derive(Subcommand)]
enum SetupTarget {
    /// Install, inspect, repair, or uninstall managed MCP for coding hosts
    #[command(name = "mcp", alias = "developer-host")]
    Mcp {
        /// Coding host to wire (claude, codex); repeat for more than one.
        /// Interactive TTY prompts when omitted; non-interactive requires --host.
        #[arg(long, value_enum)]
        host: Vec<DeveloperHost>,
        /// Print the plan only; do not install or mutate host config.
        /// Setup applies by default after the plan is displayed.
        #[arg(long)]
        dry_run: bool,
        /// Deprecated alias: setup now applies by default. Kept so existing
        /// scripts that pass `--yes` keep working. Ignored when `--dry-run` is set.
        #[arg(long, hide = true)]
        yes: bool,
        /// Single consent step for the Bicameral-server-scoped host permission
        /// grant (bot#1239): `consent` authorizes it, `skip` leaves every
        /// Bicameral MCP tool host-prompted, `ask` prompts on an interactive TTY.
        /// Never widens shell, sandbox, network, or unrelated MCP permissions.
        #[arg(long, value_enum, default_value = "ask")]
        host_permissions: HostPermissionConsent,
        #[command(subcommand)]
        action: Option<DeveloperHostAction>,
    },
}

#[derive(Subcommand)]
enum DeveloperHostAction {
    /// Inspect exact package, registration, hook, activation, and daemon readiness
    Status,
    /// Reconcile only drifted or missing setup steps
    Repair {
        /// Apply the displayed repair plan
        #[arg(long)]
        yes: bool,
    },
    /// Remove the managed hook, registration, and MCP package in that order
    Uninstall {
        /// Apply the displayed uninstall plan
        #[arg(long)]
        yes: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("bicameral=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Init { path } => commands::init::run(&path).await,
        Commands::Ingest { file } => commands::ingest::run(&file, cli.workspace.as_deref()).await,
        Commands::Preflight { target } => {
            commands::preflight::run(&target, cli.workspace.as_deref()).await
        }
        Commands::Review { action } => match action {
            ReviewAction::List => commands::review::list(cli.workspace.as_deref()).await,
            ReviewAction::Submit { id } => {
                commands::review::submit(&id, cli.workspace.as_deref()).await
            }
        },
        Commands::Mod { action } => match action {
            ModAction::Validate { manifest } => commands::mods::validate(&manifest).await,
            ModAction::Run { manifest, fixture } => commands::mods::run(&manifest, &fixture).await,
        },
        Commands::Gateway { action } => match action {
            GatewayAction::Start => commands::gateway::start(cli.workspace.as_deref()).await,
        },
        Commands::Pairing { action } => match action {
            PairingAction::Pending => commands::pairing::pending(cli.workspace.as_deref()).await,
            PairingAction::Approve => {
                commands::pairing::decide(
                    bicameral_api::hosted_bridge::PairingDecisionAction::Approve,
                    cli.workspace.as_deref(),
                )
                .await
            }
            PairingAction::Deny => {
                commands::pairing::decide(
                    bicameral_api::hosted_bridge::PairingDecisionAction::Deny,
                    cli.workspace.as_deref(),
                )
                .await
            }
        },
        Commands::Onboard { install_daemon } => commands::onboard::run(install_daemon).await,
        Commands::Dashboard => commands::dashboard::start(cli.workspace.as_deref()).await,
        Commands::Service { action } => match action {
            ServiceAction::Install => commands::service::install().await,
            ServiceAction::Start => commands::service::start().await,
            ServiceAction::Stop => commands::service::stop().await,
            ServiceAction::Status => commands::service::status().await,
            ServiceAction::Uninstall => commands::service::uninstall().await,
        },
        Commands::Setup { target } => match target {
            SetupTarget::Mcp {
                host,
                dry_run,
                yes: _yes_compat,
                host_permissions,
                action,
            } => {
                // Default setup applies after printing the plan. Plan-only needs
                // an explicit --dry-run. Legacy --yes is accepted as a no-op when
                // apply is already the default (scripts that always passed --yes).
                let operation = match action {
                    None => DeveloperHostOperation::Setup {
                        apply: commands::setup::setup_should_apply(dry_run),
                    },
                    Some(DeveloperHostAction::Status) => DeveloperHostOperation::Status,
                    Some(DeveloperHostAction::Repair { yes }) => {
                        DeveloperHostOperation::Repair { apply: yes }
                    }
                    Some(DeveloperHostAction::Uninstall { yes }) => {
                        DeveloperHostOperation::Uninstall { apply: yes }
                    }
                };
                commands::setup::run(host, operation, cli.workspace.as_deref(), host_permissions)
                    .await
            }
        },
        Commands::Doctor => commands::doctor::run(cli.workspace.as_deref()).await,
        Commands::Update { check } => commands::update::run(check).await,
        Commands::Uninstall { all } => commands::uninstall::run(all).await,
    }
}
