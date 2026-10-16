//! Bicameral-owned developer-host permission reconciliation (bot#1239).
//!
//! Locked policy: after one explicit setup consent, Bicameral configures the
//! broadest **host-native, Bicameral-server-scoped** permission the host
//! supports, and nothing else. This module never touches shell/Bash rules,
//! sandbox or approval modes, network settings, unrelated MCP servers, or
//! host-wide bypasses. It records exactly what it added so repair can
//! reconcile only that grant and uninstall can remove only that grant.
//!
//! Host permission and Bicameral action authority are separate layers: a grant
//! here only stops per-call host prompts. It never bypasses daemon-side
//! ToolRequest authorization, Product authority, confirmation, promotion,
//! signoff, correction, or governance rules, and it never produces
//! `active_verified` (see `bicameral_api::capability_readiness`).

use anyhow::{bail, Context};
use bicameral_api::capability_readiness::{
    HostPermissionMechanism, HostPermissionObservation, HostPermissionState,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::setup::DeveloperHost;

/// Claude Code permission rule that matches every tool served by the MCP
/// server registered as `bicameral` (the server-wide `mcp__<server>` form).
pub const CLAUDE_SERVER_RULE: &str = "mcp__bicameral";
/// Prefix of every per-tool Claude Code rule for the Bicameral server.
pub const CLAUDE_TOOL_RULE_PREFIX: &str = "mcp__bicameral__";
/// Documented wildcard equivalent of the server-wide rule.
pub const CLAUDE_SERVER_GLOB_RULE: &str = "mcp__bicameral__*";
/// Codex per-server key that approves every tool of one MCP server.
pub const CODEX_SERVER_APPROVAL_KEY: &str = "default_tools_approval_mode";
pub const CODEX_SERVER_APPROVAL_VALUE: &str = "approve";
/// Codex MCP server table the grant lives under. Nothing outside this table
/// is ever read for mutation or written.
pub const CODEX_SERVER_TABLE: &str = "bicameral";

const OWNED_STATE_SCHEMA: u8 = 1;

/// Keys and rule shapes that must never appear in a Bicameral grant. These are
/// checked on every write so a bug cannot widen host authority.
const FORBIDDEN_CLAUDE_RULE_PREFIXES: &[&str] = &[
    "Bash",
    "Read",
    "Edit",
    "Write",
    "WebFetch",
    "WebSearch",
    "Agent",
    "Task",
    "NotebookEdit",
];
const FORBIDDEN_CODEX_KEYS: &[&str] = &[
    "approval_policy",
    "sandbox_mode",
    "sandbox_workspace_write",
    "network_access",
    "shell_environment_policy",
    "projects",
];

/// Filesystem locations the reconciler may read or write.
#[derive(Debug, Clone)]
pub struct HostPermissionPaths {
    /// Claude Code user settings (`$CLAUDE_CONFIG_DIR/settings.json`, normally
    /// `~/.claude/settings.json`).
    pub claude_settings: PathBuf,
    /// Codex user config (`$CODEX_HOME/config.toml`, normally
    /// `~/.codex/config.toml`).
    pub codex_config: PathBuf,
    /// Bicameral-owned record of the grants it added.
    pub owned_state: PathBuf,
}

impl HostPermissionPaths {
    /// Resolve from the operator home and the Bicameral per-user share dir.
    pub fn resolve(home: &Path, share_dir: &Path) -> Self {
        let claude_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".claude"));
        let codex_dir = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| home.join(".codex"));
        Self {
            claude_settings: claude_dir.join("settings.json"),
            codex_config: codex_dir.join("config.toml"),
            owned_state: share_dir.join("mcp-host-permissions-v1.json"),
        }
    }
}

/// Host capabilities that select the grant mechanism. Both Alpha hosts expose
/// a server-scoped grant; the registry fallback exists for a host generation
/// that does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostPermissionCapabilities {
    pub server_scoped: bool,
}

impl HostPermissionCapabilities {
    pub fn for_host(host: DeveloperHost) -> Self {
        match host {
            // Claude Code: `mcp__<server>` matches every tool of that server.
            // Codex: `mcp_servers.<name>.default_tools_approval_mode`.
            DeveloperHost::Claude | DeveloperHost::Codex => Self {
                server_scoped: true,
            },
        }
    }
}

/// One declared Bicameral tool with its daemon action class, as produced by
/// the declared capability/tool registry. The reconciler never authors this
/// list; it only consumes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DeclaredTool {
    pub name: String,
    #[serde(default)]
    pub action_class: Option<String>,
}

/// Declared Bicameral capability/tool registry (from the exact installed
/// `bicameral-mcp` generation). `None` action classes cannot be allowlisted.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct DeclaredToolRegistry {
    pub tools: Vec<DeclaredTool>,
}

impl DeclaredToolRegistry {
    /// Parse the JSON registry shape (`{"tools":[{"name":..,"action_class":..}]}`
    /// or a bare array of tool objects). Plain newline-separated names (the
    /// `bicameral-mcp tools` text form) carry no action class and therefore
    /// yield tools that the fallback cannot allowlist.
    pub fn parse(text: &str) -> Self {
        let trimmed = text.trim();
        if let Ok(registry) = serde_json::from_str::<DeclaredToolRegistry>(trimmed) {
            return registry;
        }
        if let Ok(tools) = serde_json::from_str::<Vec<DeclaredTool>>(trimmed) {
            return Self { tools };
        }
        Self {
            tools: trimmed
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(|line| DeclaredTool {
                    name: line.to_string(),
                    action_class: None,
                })
                .collect(),
        }
    }
}

/// Deterministically derive the read-only/context-tool allowlist from the
/// declared registry: only tools whose declared daemon action class is
/// `read_only`, sorted and de-duplicated. Mutation, confirmation, promotion,
/// signoff, correction, governance-resolution, and unclassified tools are
/// never included. Returns `None` when the registry carries no classification
/// at all, so the caller reports `unsupported` instead of guessing.
pub fn derive_fallback_allowlist(registry: &DeclaredToolRegistry) -> Option<Vec<String>> {
    if registry.tools.is_empty() || registry.tools.iter().all(|t| t.action_class.is_none()) {
        return None;
    }
    let mut names: Vec<String> = registry
        .tools
        .iter()
        .filter(|tool| tool.action_class.as_deref() == Some("read_only"))
        .filter(|tool| tool.name.starts_with("bicameral.") && safe_tool_name(&tool.name))
        .map(|tool| tool.name.clone())
        .collect();
    names.sort();
    names.dedup();
    Some(names)
}

fn safe_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Claude Code per-tool rule for one declared tool name.
pub fn claude_tool_rule(tool_name: &str) -> String {
    format!("{CLAUDE_TOOL_RULE_PREFIX}{tool_name}")
}

// ── owned state ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedHostGrant {
    pub mechanism: HostPermissionMechanism,
    /// Exact rules/keys Bicameral added. Empty when an equivalent grant
    /// already existed and Bicameral therefore owns nothing.
    pub rules_added: Vec<String>,
    /// Codex only: the `default_tools_approval_mode` value that existed before
    /// Bicameral wrote its own, so uninstall can restore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_value: Option<String>,
    pub consented_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnedPermissionState {
    schema_version: u8,
    hosts: BTreeMap<String, OwnedHostGrant>,
}

impl Default for OwnedPermissionState {
    fn default() -> Self {
        Self {
            schema_version: OWNED_STATE_SCHEMA,
            hosts: BTreeMap::new(),
        }
    }
}

fn load_owned(paths: &HostPermissionPaths) -> anyhow::Result<OwnedPermissionState> {
    if !paths.owned_state.is_file() {
        return Ok(OwnedPermissionState::default());
    }
    let state: OwnedPermissionState = serde_json::from_slice(&fs::read(&paths.owned_state)?)
        .context("Bicameral host-permission state is invalid JSON")?;
    if state.schema_version != OWNED_STATE_SCHEMA {
        bail!(
            "unsupported Bicameral host-permission state schema {}",
            state.schema_version
        );
    }
    Ok(state)
}

fn store_owned(paths: &HostPermissionPaths, state: &OwnedPermissionState) -> anyhow::Result<()> {
    if state.hosts.is_empty() {
        if paths.owned_state.is_file() {
            fs::remove_file(&paths.owned_state)?;
        }
        return Ok(());
    }
    let parent = paths
        .owned_state
        .parent()
        .context("host-permission state path has no parent")?;
    fs::create_dir_all(parent)?;
    atomic_write(&paths.owned_state, &serde_json::to_vec_pretty(state)?)
}

/// The grant Bicameral recorded for `host`, or an error when the ownership
/// file is malformed. Callers must not treat the error as "nothing owned":
/// a malformed ownership record means Bicameral cannot know what it added,
/// so no host configuration may be mutated until the file is inspected.
pub fn owned_grant(
    paths: &HostPermissionPaths,
    host: DeveloperHost,
) -> anyhow::Result<Option<OwnedHostGrant>> {
    Ok(load_owned(paths)?.hosts.get(host_key(host)).cloned())
}

fn host_key(host: DeveloperHost) -> &'static str {
    match host {
        DeveloperHost::Claude => "claude",
        DeveloperHost::Codex => "codex",
    }
}

// ── plan / outcome ────────────────────────────────────────────────────────

/// What reconciliation would change. `NoChange` is the idempotent steady state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionPlan {
    NoChange,
    /// Add exactly these Claude rules / set this Codex key.
    Grant {
        rules: Vec<String>,
    },
    /// The host configuration is malformed; Bicameral refuses to rewrite it.
    Malformed {
        reason: String,
    },
    /// Bicameral's own ownership record is malformed. Nothing is written to
    /// any host until the record is inspected; the file is preserved as-is.
    OwnershipStateMalformed {
        reason: String,
    },
    /// A stricter existing host policy already covers the Bicameral server;
    /// Bicameral preserves it and adds nothing.
    StricterPolicyPreserved {
        reason: String,
    },
    /// No server-scoped grant and no classified registry: tools stay prompted.
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileOutcome {
    pub observation: HostPermissionObservation,
    pub plan: PermissionPlan,
    pub applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalOutcome {
    /// Rules Bicameral removed because they were still exactly its own.
    pub removed: Vec<String>,
    /// Rules Bicameral owned but left in place because the operator changed
    /// the surrounding configuration since the grant (never clobbered).
    /// Ownership metadata for these rules is retained so a later status,
    /// repair, or uninstall still knows they originated from Bicameral.
    pub preserved_operator_changes: Vec<String>,
    /// Rules Bicameral owned that the operator had already removed; nothing
    /// of Bicameral's survives for them, so their ownership record is dropped.
    #[allow(dead_code)]
    pub absent: Vec<String>,
}

// ── observation ───────────────────────────────────────────────────────────

/// Observe the current permission state for `host` without mutating anything.
pub fn observe(
    host: DeveloperHost,
    paths: &HostPermissionPaths,
    capabilities: HostPermissionCapabilities,
) -> HostPermissionObservation {
    let owned = match owned_grant(paths, host) {
        Ok(owned) => owned,
        // Bicameral cannot tell what it owns: report drift, never mutate.
        Err(_) => {
            return HostPermissionObservation {
                state: HostPermissionState::Drifted,
                mechanism: None,
                owned_rules: Vec::new(),
            }
        }
    };
    let (present, malformed) = match host {
        DeveloperHost::Claude => match read_claude_allow_rules(&paths.claude_settings) {
            // A grant counts only when it is effective: a covering deny or ask
            // rule wins over allow, so the Bicameral grant is not configured
            // even when the allow rule is present.
            Ok(rules) => (claude_grant_effective(&rules, owned.as_ref()), false),
            Err(_) => (false, true),
        },
        DeveloperHost::Codex => match read_codex_document(&paths.codex_config) {
            Ok(Some(document)) => (codex_grant_present(&document, owned.as_ref()), false),
            Ok(None) => (false, false),
            Err(_) => (false, true),
        },
    };
    let mechanism = owned
        .as_ref()
        .map(|grant| grant.mechanism)
        .or(if capabilities.server_scoped {
            Some(HostPermissionMechanism::ServerScoped)
        } else {
            None
        });
    let state = match (present, malformed, &owned) {
        (true, _, _) => HostPermissionState::Configured,
        (false, true, _) => HostPermissionState::Drifted,
        (false, false, Some(grant)) if !grant.rules_added.is_empty() => {
            HostPermissionState::Drifted
        }
        (false, false, _) if !capabilities.server_scoped => HostPermissionState::Unsupported,
        _ => HostPermissionState::Missing,
    };
    HostPermissionObservation {
        state,
        mechanism,
        owned_rules: owned.map(|grant| grant.rules_added).unwrap_or_default(),
    }
}

// ── reconcile (setup / repair) ────────────────────────────────────────────

/// Reconcile the Bicameral grant for `host`.
///
/// * `consent == false` → observe only; nothing is written.
/// * Already correct → `NoChange`, nothing written (idempotent repair).
/// * Otherwise the minimum scoped change is applied: one Claude allow rule or
///   one Codex per-server key. Stricter existing policy, unrelated settings,
///   and malformed files are preserved untouched.
pub fn reconcile(
    host: DeveloperHost,
    paths: &HostPermissionPaths,
    capabilities: HostPermissionCapabilities,
    consent: bool,
    registry: Option<&DeclaredToolRegistry>,
    now: &str,
) -> anyhow::Result<ReconcileOutcome> {
    let plan = plan(host, paths, capabilities, registry)?;
    if !consent || !matches!(plan, PermissionPlan::Grant { .. }) {
        return Ok(ReconcileOutcome {
            observation: observe(host, paths, capabilities),
            plan,
            applied: false,
        });
    }
    let PermissionPlan::Grant { rules } = &plan else {
        unreachable!()
    };
    let mechanism = if capabilities.server_scoped {
        HostPermissionMechanism::ServerScoped
    } else {
        HostPermissionMechanism::RegistryFallback
    };
    let prior_value = match host {
        DeveloperHost::Claude => {
            apply_claude_grant(&paths.claude_settings, rules)?;
            None
        }
        DeveloperHost::Codex => apply_codex_grant(&paths.codex_config, rules)?,
    };
    let mut owned = load_owned(paths)?;
    owned.hosts.insert(
        host_key(host).to_string(),
        OwnedHostGrant {
            mechanism,
            rules_added: rules.clone(),
            prior_value,
            consented_at: now.to_string(),
        },
    );
    store_owned(paths, &owned)?;
    Ok(ReconcileOutcome {
        observation: observe(host, paths, capabilities),
        plan,
        applied: true,
    })
}

/// Compute the minimum change without writing.
pub fn plan(
    host: DeveloperHost,
    paths: &HostPermissionPaths,
    capabilities: HostPermissionCapabilities,
    registry: Option<&DeclaredToolRegistry>,
) -> anyhow::Result<PermissionPlan> {
    let owned = match owned_grant(paths, host) {
        Ok(owned) => owned,
        Err(error) => {
            return Ok(PermissionPlan::OwnershipStateMalformed {
                reason: error.to_string(),
            })
        }
    };
    let desired = desired_rules(host, capabilities, registry);
    let desired = match desired {
        Ok(rules) => rules,
        Err(reason) => return Ok(PermissionPlan::Unsupported { reason }),
    };
    match host {
        DeveloperHost::Claude => {
            let rules = match read_claude_allow_rules(&paths.claude_settings) {
                Ok(rules) => rules,
                Err(error) => {
                    return Ok(PermissionPlan::Malformed {
                        reason: error.to_string(),
                    })
                }
            };
            if let Some(reason) = claude_stricter_policy(&paths.claude_settings)? {
                return Ok(PermissionPlan::StricterPolicyPreserved { reason });
            }
            if claude_grant_effective(&rules, owned.as_ref()) {
                return Ok(PermissionPlan::NoChange);
            }
            let missing: Vec<String> = desired
                .into_iter()
                .filter(|rule| !rules.allow.iter().any(|existing| existing == rule))
                .collect();
            if missing.is_empty() {
                return Ok(PermissionPlan::NoChange);
            }
            Ok(PermissionPlan::Grant { rules: missing })
        }
        DeveloperHost::Codex => {
            let document = match read_codex_document(&paths.codex_config) {
                Ok(Some(document)) => document,
                Ok(None) => {
                    return Ok(PermissionPlan::StricterPolicyPreserved {
                        reason: "codex config has no [mcp_servers.bicameral] table yet; \
                                 register the server first"
                            .into(),
                    })
                }
                Err(error) => {
                    return Ok(PermissionPlan::Malformed {
                        reason: error.to_string(),
                    })
                }
            };
            if codex_grant_present(&document, owned.as_ref()) {
                return Ok(PermissionPlan::NoChange);
            }
            if let Some(reason) = codex_stricter_policy(&document, owned.as_ref()) {
                return Ok(PermissionPlan::StricterPolicyPreserved { reason });
            }
            Ok(PermissionPlan::Grant { rules: desired })
        }
    }
}

fn desired_rules(
    host: DeveloperHost,
    capabilities: HostPermissionCapabilities,
    registry: Option<&DeclaredToolRegistry>,
) -> Result<Vec<String>, String> {
    if capabilities.server_scoped {
        return Ok(match host {
            DeveloperHost::Claude => vec![CLAUDE_SERVER_RULE.to_string()],
            DeveloperHost::Codex => vec![format!(
                "{CODEX_SERVER_APPROVAL_KEY}={CODEX_SERVER_APPROVAL_VALUE}"
            )],
        });
    }
    let Some(registry) = registry else {
        return Err(
            "host exposes no server-scoped grant and no declared tool registry \
                    was available; Bicameral tools stay host-prompted"
                .into(),
        );
    };
    let Some(tools) = derive_fallback_allowlist(registry) else {
        return Err(
            "declared tool registry carries no action classes; Bicameral tools \
                    stay host-prompted"
                .into(),
        );
    };
    if tools.is_empty() {
        return Err("declared tool registry has no read-only tools to allowlist".into());
    }
    Ok(match host {
        DeveloperHost::Claude => tools.iter().map(|name| claude_tool_rule(name)).collect(),
        DeveloperHost::Codex => tools
            .iter()
            .map(|name| format!("tools.{name}.approval_mode={CODEX_SERVER_APPROVAL_VALUE}"))
            .collect(),
    })
}

// ── removal (uninstall) ───────────────────────────────────────────────────

/// Remove only the grant Bicameral recorded as its own. Rules the operator
/// added, changed, or that pre-existed are never touched. If the operator has
/// since altered Bicameral's own entry, it is preserved and reported.
///
/// Ownership metadata is dropped only for elements that were actually removed
/// or restored, or that the operator had already removed. Elements that must
/// be preserved (operator-modified value, malformed host file) keep their
/// ownership record so Bicameral still knows a surviving permission originated
/// from Bicameral. A malformed ownership record is an error: nothing is
/// written and the record is left in place for inspection.
pub fn remove(host: DeveloperHost, paths: &HostPermissionPaths) -> anyhow::Result<RemovalOutcome> {
    let mut owned = load_owned(paths)?;
    let Some(grant) = owned.hosts.get(host_key(host)).cloned() else {
        return Ok(RemovalOutcome {
            removed: Vec::new(),
            preserved_operator_changes: Vec::new(),
            absent: Vec::new(),
        });
    };
    let mut outcome = match host {
        DeveloperHost::Claude => remove_claude_grant(&paths.claude_settings, &grant)?,
        DeveloperHost::Codex => remove_codex_grant(&paths.codex_config, &grant)?,
    };
    outcome.absent = grant
        .rules_added
        .iter()
        .filter(|rule| {
            !outcome.removed.contains(rule) && !outcome.preserved_operator_changes.contains(rule)
        })
        .cloned()
        .collect();
    let remaining: Vec<String> = grant
        .rules_added
        .iter()
        .filter(|rule| outcome.preserved_operator_changes.contains(rule))
        .cloned()
        .collect();
    if remaining.is_empty() {
        owned.hosts.remove(host_key(host));
    } else {
        let keeps_server_key = remaining
            .iter()
            .any(|rule| rule.starts_with(CODEX_SERVER_APPROVAL_KEY));
        owned.hosts.insert(
            host_key(host).to_string(),
            OwnedHostGrant {
                mechanism: grant.mechanism,
                rules_added: remaining,
                prior_value: if keeps_server_key {
                    grant.prior_value.clone()
                } else {
                    None
                },
                consented_at: grant.consented_at.clone(),
            },
        );
    }
    store_owned(paths, &owned)?;
    Ok(outcome)
}

// ── Claude Code ───────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct ClaudeRules {
    allow: Vec<String>,
    deny: Vec<String>,
    ask: Vec<String>,
}

fn read_claude_settings(path: &Path) -> anyhow::Result<serde_json::Value> {
    if !path.is_file() {
        return Ok(serde_json::json!({}));
    }
    let bytes = fs::read(path)?;
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(serde_json::json!({}));
    }
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).context("Claude settings.json is not valid JSON")?;
    if !value.is_object() {
        bail!("Claude settings.json is not a JSON object");
    }
    Ok(value)
}

fn rule_list(value: &serde_json::Value, key: &str) -> anyhow::Result<Vec<String>> {
    match value.get("permissions").and_then(|p| p.get(key)) {
        None | Some(serde_json::Value::Null) => Ok(Vec::new()),
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(str::to_string)
                    .context("Claude permission rule is not a string")
            })
            .collect(),
        Some(_) => bail!("Claude permissions.{key} is not an array"),
    }
}

fn read_claude_allow_rules(path: &Path) -> anyhow::Result<ClaudeRules> {
    let value = read_claude_settings(path)?;
    if let Some(permissions) = value.get("permissions") {
        if !permissions.is_object() {
            bail!("Claude permissions is not an object");
        }
    }
    Ok(ClaudeRules {
        allow: rule_list(&value, "allow")?,
        deny: rule_list(&value, "deny")?,
        ask: rule_list(&value, "ask")?,
    })
}

fn rule_targets_bicameral(rule: &str) -> bool {
    rule == CLAUDE_SERVER_RULE || rule.starts_with(CLAUDE_TOOL_RULE_PREFIX)
}

fn claude_server_wide(rule: &str) -> bool {
    rule == CLAUDE_SERVER_RULE || rule == CLAUDE_SERVER_GLOB_RULE
}

/// Deny/ask globs that already cover every Bicameral tool.
fn claude_covering_restriction(rule: &str) -> bool {
    claude_server_wide(rule) || rule == "mcp__*" || rule == "*"
}

/// Whether a covering deny or ask rule makes any Bicameral allow rule
/// ineffective. Deny and ask are evaluated before allow by Claude Code.
fn claude_restricted(rules: &ClaudeRules) -> bool {
    rules
        .deny
        .iter()
        .chain(rules.ask.iter())
        .any(|rule| claude_covering_restriction(rule))
}

/// The Bicameral grant is configured only when it is present *and* effective.
fn claude_grant_effective(rules: &ClaudeRules, owned: Option<&OwnedHostGrant>) -> bool {
    claude_grant_present(rules, owned) && !claude_restricted(rules)
}

fn claude_grant_present(rules: &ClaudeRules, owned: Option<&OwnedHostGrant>) -> bool {
    if rules.allow.iter().any(|rule| claude_server_wide(rule)) {
        return true;
    }
    match owned {
        Some(grant) if !grant.rules_added.is_empty() => grant
            .rules_added
            .iter()
            .all(|rule| rules.allow.iter().any(|existing| existing == rule)),
        _ => false,
    }
}

/// A server-wide deny or ask rule for Bicameral is stricter policy that wins.
fn claude_stricter_policy(path: &Path) -> anyhow::Result<Option<String>> {
    let rules = read_claude_allow_rules(path)?;
    if let Some(rule) = rules
        .deny
        .iter()
        .find(|rule| claude_covering_restriction(rule))
    {
        return Ok(Some(format!(
            "permissions.deny already contains {rule}; Bicameral preserves it"
        )));
    }
    if let Some(rule) = rules
        .ask
        .iter()
        .find(|rule| claude_covering_restriction(rule))
    {
        return Ok(Some(format!(
            "permissions.ask already contains {rule}; Bicameral preserves it"
        )));
    }
    Ok(None)
}

fn assert_claude_rules_scoped(rules: &[String]) -> anyhow::Result<()> {
    for rule in rules {
        if !rule_targets_bicameral(rule) {
            bail!("refusing to add a non-Bicameral Claude permission rule: {rule}");
        }
        if rule != CLAUDE_SERVER_RULE {
            let tool = rule
                .strip_prefix(CLAUDE_TOOL_RULE_PREFIX)
                .unwrap_or_default();
            if !(tool.starts_with("bicameral.") && safe_tool_name(tool)) {
                bail!("refusing to add a malformed Bicameral tool permission rule: {rule}");
            }
        }
        if FORBIDDEN_CLAUDE_RULE_PREFIXES
            .iter()
            .any(|prefix| rule.starts_with(prefix))
        {
            bail!("refusing to add a host-authority Claude permission rule: {rule}");
        }
    }
    Ok(())
}

fn apply_claude_grant(path: &Path, rules: &[String]) -> anyhow::Result<()> {
    assert_claude_rules_scoped(rules)?;
    let mut value = read_claude_settings(path)?;
    let root = value
        .as_object_mut()
        .context("Claude settings.json is not a JSON object")?;
    let permissions = root
        .entry("permissions")
        .or_insert_with(|| serde_json::json!({}));
    let permissions = permissions
        .as_object_mut()
        .context("Claude permissions is not an object")?;
    let allow = permissions
        .entry("allow")
        .or_insert_with(|| serde_json::json!([]));
    let allow = allow
        .as_array_mut()
        .context("Claude permissions.allow is not an array")?;
    for rule in rules {
        if !allow.iter().any(|existing| existing.as_str() == Some(rule)) {
            allow.push(serde_json::Value::String(rule.clone()));
        }
    }
    write_claude_settings(path, &value)
}

fn remove_claude_grant(path: &Path, grant: &OwnedHostGrant) -> anyhow::Result<RemovalOutcome> {
    let mut outcome = RemovalOutcome {
        removed: Vec::new(),
        preserved_operator_changes: Vec::new(),
        absent: Vec::new(),
    };
    if grant.rules_added.is_empty() || !path.is_file() {
        return Ok(outcome);
    }
    let mut value = match read_claude_settings(path) {
        Ok(value) => value,
        Err(_) => {
            // Malformed since the grant: never rewrite.
            outcome.preserved_operator_changes = grant.rules_added.clone();
            return Ok(outcome);
        }
    };
    let Some(allow) = value
        .get_mut("permissions")
        .and_then(|p| p.get_mut("allow"))
        .and_then(|a| a.as_array_mut())
    else {
        return Ok(outcome);
    };
    for rule in &grant.rules_added {
        let before = allow.len();
        allow.retain(|existing| existing.as_str() != Some(rule));
        if allow.len() < before {
            outcome.removed.push(rule.clone());
        }
    }
    if outcome.removed.is_empty() {
        return Ok(outcome);
    }
    write_claude_settings(path, &value)?;
    Ok(outcome)
}

fn write_claude_settings(path: &Path, value: &serde_json::Value) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .context("Claude settings path has no parent")?;
    fs::create_dir_all(parent)?;
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

// ── Codex ─────────────────────────────────────────────────────────────────

fn read_codex_document(path: &Path) -> anyhow::Result<Option<toml_edit::DocumentMut>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path)?;
    let document: toml_edit::DocumentMut = text
        .parse()
        .context("Codex config.toml is not valid TOML")?;
    if codex_server_table(&document).is_none() {
        return Ok(None);
    }
    Ok(Some(document))
}

fn codex_server_table(document: &toml_edit::DocumentMut) -> Option<&toml_edit::Table> {
    document
        .get("mcp_servers")
        .and_then(|servers| servers.as_table())
        .and_then(|servers| servers.get(CODEX_SERVER_TABLE))
        .and_then(|server| server.as_table())
}

fn codex_server_table_mut(document: &mut toml_edit::DocumentMut) -> Option<&mut toml_edit::Table> {
    document
        .get_mut("mcp_servers")
        .and_then(|servers| servers.as_table_mut())
        .and_then(|servers| servers.get_mut(CODEX_SERVER_TABLE))
        .and_then(|server| server.as_table_mut())
}

fn codex_current_mode(document: &toml_edit::DocumentMut) -> Option<String> {
    codex_server_table(document)
        .and_then(|table| table.get(CODEX_SERVER_APPROVAL_KEY))
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

fn codex_grant_present(document: &toml_edit::DocumentMut, owned: Option<&OwnedHostGrant>) -> bool {
    if codex_current_mode(document).as_deref() == Some(CODEX_SERVER_APPROVAL_VALUE) {
        return true;
    }
    match owned {
        Some(grant)
            if grant.mechanism == HostPermissionMechanism::RegistryFallback
                && !grant.rules_added.is_empty() =>
        {
            let Some(table) = codex_server_table(document) else {
                return false;
            };
            grant.rules_added.iter().all(|rule| {
                codex_tool_rule(rule).is_some_and(|(tool, mode)| {
                    table
                        .get("tools")
                        .and_then(|tools| tools.as_table())
                        .and_then(|tools| tools.get(&tool))
                        .and_then(|entry| entry.as_table())
                        .and_then(|entry| entry.get("approval_mode"))
                        .and_then(|item| item.as_str())
                        == Some(mode.as_str())
                })
            })
        }
        _ => false,
    }
}

/// An explicit non-approve server mode that Bicameral did not write is the
/// operator's stricter policy.
fn codex_stricter_policy(
    document: &toml_edit::DocumentMut,
    owned: Option<&OwnedHostGrant>,
) -> Option<String> {
    let current = codex_current_mode(document)?;
    if current == CODEX_SERVER_APPROVAL_VALUE {
        return None;
    }
    let ours = owned.is_some_and(|grant| {
        grant
            .rules_added
            .iter()
            .any(|rule| rule.starts_with(CODEX_SERVER_APPROVAL_KEY))
    });
    if ours {
        // We wrote approve and the operator tightened it: drift we respect.
        return Some(format!(
            "operator changed mcp_servers.{CODEX_SERVER_TABLE}.{CODEX_SERVER_APPROVAL_KEY} to \
             \"{current}\" after setup; Bicameral preserves it"
        ));
    }
    Some(format!(
        "mcp_servers.{CODEX_SERVER_TABLE}.{CODEX_SERVER_APPROVAL_KEY} = \"{current}\" is existing \
         host policy; Bicameral preserves it"
    ))
}

fn codex_tool_rule(rule: &str) -> Option<(String, String)> {
    let rest = rule.strip_prefix("tools.")?;
    let (tool, value) = rest.split_once(".approval_mode=")?;
    Some((tool.to_string(), value.to_string()))
}

fn assert_codex_rules_scoped(rules: &[String]) -> anyhow::Result<()> {
    for rule in rules {
        let key = rule.split('=').next().unwrap_or_default();
        let top = key.split('.').next().unwrap_or_default();
        if FORBIDDEN_CODEX_KEYS.contains(&top) {
            bail!("refusing to write a host-authority Codex key: {rule}");
        }
        let ok = rule == &format!("{CODEX_SERVER_APPROVAL_KEY}={CODEX_SERVER_APPROVAL_VALUE}")
            || codex_tool_rule(rule).is_some_and(|(tool, mode)| {
                tool.starts_with("bicameral.")
                    && safe_tool_name(&tool)
                    && mode == CODEX_SERVER_APPROVAL_VALUE
            });
        if !ok {
            bail!("refusing to write a non-Bicameral Codex approval rule: {rule}");
        }
    }
    Ok(())
}

/// Returns the prior server approval value (if any) so uninstall can restore it.
fn apply_codex_grant(path: &Path, rules: &[String]) -> anyhow::Result<Option<String>> {
    assert_codex_rules_scoped(rules)?;
    let mut document = read_codex_document(path)?
        .context("codex config has no [mcp_servers.bicameral] table; register the server first")?;
    let prior = codex_current_mode(&document);
    {
        let table = codex_server_table_mut(&mut document)
            .context("codex [mcp_servers.bicameral] table vanished during reconcile")?;
        for rule in rules {
            if rule == &format!("{CODEX_SERVER_APPROVAL_KEY}={CODEX_SERVER_APPROVAL_VALUE}") {
                table[CODEX_SERVER_APPROVAL_KEY] = toml_edit::value(CODEX_SERVER_APPROVAL_VALUE);
            } else if let Some((tool, mode)) = codex_tool_rule(rule) {
                let tools = table
                    .entry("tools")
                    .or_insert(toml_edit::table())
                    .as_table_mut()
                    .context("codex [mcp_servers.bicameral.tools] is not a table")?;
                let entry = tools
                    .entry(&tool)
                    .or_insert(toml_edit::table())
                    .as_table_mut()
                    .context("codex per-tool approval entry is not a table")?;
                entry["approval_mode"] = toml_edit::value(mode);
            }
        }
    }
    write_codex_document(path, &document)?;
    Ok(prior)
}

fn remove_codex_grant(path: &Path, grant: &OwnedHostGrant) -> anyhow::Result<RemovalOutcome> {
    let mut outcome = RemovalOutcome {
        removed: Vec::new(),
        preserved_operator_changes: Vec::new(),
        absent: Vec::new(),
    };
    if grant.rules_added.is_empty() || !path.is_file() {
        return Ok(outcome);
    }
    let mut document = match read_codex_document(path) {
        Ok(Some(document)) => document,
        Ok(None) => return Ok(outcome),
        Err(_) => {
            outcome.preserved_operator_changes = grant.rules_added.clone();
            return Ok(outcome);
        }
    };
    let mut changed = false;
    {
        let Some(table) = codex_server_table_mut(&mut document) else {
            return Ok(outcome);
        };
        for rule in &grant.rules_added {
            if rule == &format!("{CODEX_SERVER_APPROVAL_KEY}={CODEX_SERVER_APPROVAL_VALUE}") {
                let current = table
                    .get(CODEX_SERVER_APPROVAL_KEY)
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
                if current.as_deref() == Some(CODEX_SERVER_APPROVAL_VALUE) {
                    match &grant.prior_value {
                        Some(prior) => table[CODEX_SERVER_APPROVAL_KEY] = toml_edit::value(prior),
                        None => {
                            table.remove(CODEX_SERVER_APPROVAL_KEY);
                        }
                    }
                    changed = true;
                    outcome.removed.push(rule.clone());
                } else if current.is_some() {
                    outcome.preserved_operator_changes.push(rule.clone());
                }
            } else if let Some((tool, mode)) = codex_tool_rule(rule) {
                let current = table
                    .get("tools")
                    .and_then(|tools| tools.as_table())
                    .and_then(|tools| tools.get(&tool))
                    .and_then(|entry| entry.as_table())
                    .and_then(|entry| entry.get("approval_mode"))
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
                if current.as_deref() == Some(mode.as_str()) {
                    if let Some(tools) = table.get_mut("tools").and_then(|t| t.as_table_mut()) {
                        tools.remove(&tool);
                        if tools.is_empty() {
                            table.remove("tools");
                        }
                    }
                    changed = true;
                    outcome.removed.push(rule.clone());
                } else if current.is_some() {
                    outcome.preserved_operator_changes.push(rule.clone());
                }
            }
        }
    }
    if changed {
        write_codex_document(path, &document)?;
    }
    Ok(outcome)
}

fn write_codex_document(path: &Path, document: &toml_edit::DocumentMut) -> anyhow::Result<()> {
    atomic_write(path, document.to_string().as_bytes())
}

// ── shared ────────────────────────────────────────────────────────────────

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("target path has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.bicameral-tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));
    // No persistent full-file backup is ever left behind: rollback uses the
    // owned-grant record, and the temporary file is removed on any failure.
    let result = (|| -> anyhow::Result<()> {
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
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        #[cfg(not(unix))]
        fs::write(&temporary, bytes)?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Human-readable, secret-free summary for the setup plan output.
pub fn describe_plan(host: DeveloperHost, plan: &PermissionPlan) -> String {
    match plan {
        PermissionPlan::NoChange => "host permission: Bicameral grant already configured".into(),
        PermissionPlan::Grant { rules } => match host {
            DeveloperHost::Claude => format!(
                "host permission: add Claude Code allow rule(s) {} to user settings (no shell, \
                 sandbox, network, or other MCP rules)",
                rules.join(", ")
            ),
            DeveloperHost::Codex => format!(
                "host permission: set {} under [mcp_servers.{CODEX_SERVER_TABLE}] only (no \
                 approval_policy, sandbox, network, or other MCP changes)",
                rules.join(", ")
            ),
        },
        PermissionPlan::Malformed { reason } => {
            format!("host permission: host configuration is malformed; no rewrite ({reason})")
        }
        PermissionPlan::OwnershipStateMalformed { reason } => format!(
            "host permission: Bicameral ownership record is malformed; no host configuration \
             will be changed until it is inspected ({reason})"
        ),
        PermissionPlan::StricterPolicyPreserved { reason } => {
            format!("host permission: existing host policy preserved ({reason})")
        }
        PermissionPlan::Unsupported { reason } => {
            format!("host permission: unsupported on this host generation ({reason})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const NOW: &str = "2026-09-10T12:00:00Z";

    fn paths(root: &TempDir) -> HostPermissionPaths {
        HostPermissionPaths {
            claude_settings: root.path().join("claude/settings.json"),
            codex_config: root.path().join("codex/config.toml"),
            owned_state: root.path().join("share/mcp-host-permissions-v1.json"),
        }
    }

    fn server_scoped() -> HostPermissionCapabilities {
        HostPermissionCapabilities {
            server_scoped: true,
        }
    }

    fn fallback_only() -> HostPermissionCapabilities {
        HostPermissionCapabilities {
            server_scoped: false,
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn claude_json(path: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
    }

    const CLAUDE_BASELINE: &str = r#"{
  "model": "opus",
  "permissions": {
    "allow": ["Bash(git status:*)", "mcp__github__list_issues"],
    "deny": ["Bash(rm -rf:*)", "mcp__bicameral__bicameral.privacy.erase_subject"],
    "ask": ["WebFetch"],
    "defaultMode": "default",
    "additionalDirectories": ["/srv/shared"]
  },
  "enableAllProjectMcpServers": false,
  "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "bicameral-mcp prework-run"}]}]},
  "env": {"NETWORK_PROXY": "http://proxy.local:3128"}
}
"#;

    const CODEX_BASELINE: &str = r#"# operator config
model = "gpt-5-codex"
approval_policy = "on-request"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
network_access = false

[mcp_servers.github]
command = "github-mcp"
default_tools_approval_mode = "prompt"

[mcp_servers.bicameral]
command = "/home/op/.local/share/bicameral/bin/bicameral-mcp"
enabled = true

[mcp_servers.bicameral.tools."bicameral.privacy.erase_subject"]
approval_mode = "prompt"

[projects."/home/op/repo"]
trust_level = "trusted"
"#;

    // ── Claude Code ───────────────────────────────────────────────────────

    #[test]
    fn claude_consented_setup_adds_only_the_server_scoped_rule() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        let before = claude_json(&p.claude_settings);

        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(outcome.applied);
        assert_eq!(
            outcome.plan,
            PermissionPlan::Grant {
                rules: vec![CLAUDE_SERVER_RULE.to_string()]
            }
        );
        assert_eq!(outcome.observation.state, HostPermissionState::Configured);
        assert_eq!(
            outcome.observation.mechanism,
            Some(HostPermissionMechanism::ServerScoped)
        );
        assert_eq!(outcome.observation.owned_rules, vec![CLAUDE_SERVER_RULE]);

        let after = claude_json(&p.claude_settings);
        let allow = after["permissions"]["allow"].as_array().unwrap();
        assert_eq!(allow.len(), 3);
        assert_eq!(allow[2], CLAUDE_SERVER_RULE);
        // Everything except the one appended allow rule is byte-for-byte the
        // same JSON value: Bash rules, deny, ask, defaultMode, other MCP
        // servers, hooks, env, and additional directories are untouched.
        let mut expected = before.clone();
        expected["permissions"]["allow"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String(CLAUDE_SERVER_RULE.into()));
        assert_eq!(after, expected);
        assert_eq!(after["permissions"]["deny"], before["permissions"]["deny"]);
        assert_eq!(after["permissions"]["ask"], before["permissions"]["ask"]);
        assert_eq!(after["permissions"]["defaultMode"], "default");
        assert!(after
            .to_string()
            .contains("mcp__bicameral__bicameral.privacy.erase_subject"));
        assert!(!after.to_string().contains("bypassPermissions"));
        // No persistent full-config backup or temp file is left behind.
        assert_no_backups(root.path());
    }

    #[test]
    fn claude_setup_without_consent_writes_nothing_and_reports_missing() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);

        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), false, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert_eq!(outcome.observation.state, HostPermissionState::Missing);
        assert_eq!(
            fs::read_to_string(&p.claude_settings).unwrap(),
            CLAUDE_BASELINE
        );
        assert!(!p.owned_state.exists());
    }

    #[test]
    fn claude_repair_is_idempotent_on_correct_state() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        let written = fs::read(&p.claude_settings).unwrap();
        let owned = fs::read(&p.owned_state).unwrap();
        let mtime = fs::metadata(&p.claude_settings)
            .unwrap()
            .modified()
            .unwrap();

        let second =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!second.applied);
        assert_eq!(second.plan, PermissionPlan::NoChange);
        assert_eq!(second.observation.state, HostPermissionState::Configured);
        assert_eq!(fs::read(&p.claude_settings).unwrap(), written);
        assert_eq!(fs::read(&p.owned_state).unwrap(), owned);
        assert_eq!(
            fs::metadata(&p.claude_settings)
                .unwrap()
                .modified()
                .unwrap(),
            mtime
        );
        assert_no_backups(root.path());
    }

    #[test]
    fn claude_missing_settings_file_gets_only_the_permission_block() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);

        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(outcome.applied);
        assert_eq!(
            claude_json(&p.claude_settings),
            serde_json::json!({"permissions": {"allow": [CLAUDE_SERVER_RULE]}})
        );
    }

    #[test]
    fn claude_stricter_deny_or_ask_policy_wins_and_is_preserved() {
        for (key, rule) in [
            ("deny", CLAUDE_SERVER_RULE),
            ("ask", CLAUDE_SERVER_RULE),
            ("deny", CLAUDE_SERVER_GLOB_RULE),
            ("deny", "mcp__*"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let p = paths(&root);
            let text =
                format!(r#"{{"permissions": {{"{key}": ["{rule}"], "allow": ["Bash(ls:*)"]}}}}"#);
            write(&p.claude_settings, &text);

            let outcome =
                reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

            assert!(!outcome.applied, "{key}");
            assert!(matches!(
                outcome.plan,
                PermissionPlan::StricterPolicyPreserved { .. }
            ));
            assert_eq!(outcome.observation.state, HostPermissionState::Missing);
            assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), text);
            assert!(!p.owned_state.exists());
        }
    }

    #[test]
    fn claude_preexisting_operator_grant_is_recognized_but_not_owned() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(
            &p.claude_settings,
            r#"{"permissions": {"allow": ["mcp__bicameral", "Bash(ls:*)"]}}"#,
        );

        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert_eq!(outcome.plan, PermissionPlan::NoChange);
        assert_eq!(outcome.observation.state, HostPermissionState::Configured);
        assert!(outcome.observation.owned_rules.is_empty());
        // Uninstall therefore removes nothing.
        let removal = remove(DeveloperHost::Claude, &p).unwrap();
        assert!(removal.removed.is_empty());
        let after = claude_json(&p.claude_settings);
        assert_eq!(after["permissions"]["allow"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn claude_malformed_settings_are_never_rewritten() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, "{ not json");

        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert!(matches!(outcome.plan, PermissionPlan::Malformed { .. }));
        assert_eq!(outcome.observation.state, HostPermissionState::Drifted);
        assert_eq!(
            fs::read_to_string(&p.claude_settings).unwrap(),
            "{ not json"
        );
    }

    #[test]
    fn claude_uninstall_removes_only_the_bicameral_owned_rule() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        let before = claude_json(&p.claude_settings);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        let removal = remove(DeveloperHost::Claude, &p).unwrap();

        assert_eq!(removal.removed, vec![CLAUDE_SERVER_RULE]);
        assert!(removal.preserved_operator_changes.is_empty());
        assert_eq!(claude_json(&p.claude_settings), before);
        assert!(!p.owned_state.exists());
        assert_eq!(
            observe(DeveloperHost::Claude, &p, server_scoped()).state,
            HostPermissionState::Missing
        );
    }

    #[test]
    fn claude_uninstall_does_not_clobber_operator_changes_after_setup() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        // Operator later removes our rule and adds their own narrower rule.
        let mut edited = claude_json(&p.claude_settings);
        edited["permissions"]["allow"] = serde_json::json!([
            "Bash(git status:*)",
            "mcp__github__list_issues",
            "mcp__bicameral__bicameral.context"
        ]);
        write(
            &p.claude_settings,
            &serde_json::to_string_pretty(&edited).unwrap(),
        );
        assert_eq!(
            observe(DeveloperHost::Claude, &p, server_scoped()).state,
            HostPermissionState::Drifted
        );

        let removal = remove(DeveloperHost::Claude, &p).unwrap();

        assert!(removal.removed.is_empty());
        assert_eq!(claude_json(&p.claude_settings), edited);
        assert!(!p.owned_state.exists());
    }

    #[test]
    fn claude_uninstall_leaves_a_malformed_file_alone() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        write(&p.claude_settings, "{ broken");

        let removal = remove(DeveloperHost::Claude, &p).unwrap();

        assert!(removal.removed.is_empty());
        assert_eq!(removal.preserved_operator_changes, vec![CLAUDE_SERVER_RULE]);
        assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), "{ broken");
    }

    #[test]
    fn claude_writer_refuses_any_non_bicameral_or_host_authority_rule() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        for rule in [
            "Bash(*)",
            "Bash",
            "mcp__github",
            "WebFetch",
            "Read",
            "mcp__bicameral__../Bash",
        ] {
            let error = apply_claude_grant(&p.claude_settings, &[rule.to_string()]).unwrap_err();
            assert!(error.to_string().contains("refusing"), "{rule}: {error}");
        }
        assert!(!p.claude_settings.exists());
    }

    // ── Codex ─────────────────────────────────────────────────────────────

    #[test]
    fn codex_consented_setup_sets_only_the_bicameral_server_approval_mode() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);

        let outcome =
            reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(outcome.applied);
        assert_eq!(outcome.observation.state, HostPermissionState::Configured);
        assert_eq!(
            outcome.observation.owned_rules,
            vec!["default_tools_approval_mode=approve"]
        );
        let after = fs::read_to_string(&p.codex_config).unwrap();
        let document: toml_edit::DocumentMut = after.parse().unwrap();
        assert_eq!(
            document["mcp_servers"]["bicameral"]["default_tools_approval_mode"].as_str(),
            Some("approve")
        );
        // Global approval, sandbox, network, unrelated MCP, per-tool stricter
        // override, comments, and project trust are untouched.
        assert_eq!(document["approval_policy"].as_str(), Some("on-request"));
        assert_eq!(document["sandbox_mode"].as_str(), Some("workspace-write"));
        assert_eq!(
            document["sandbox_workspace_write"]["network_access"].as_bool(),
            Some(false)
        );
        assert_eq!(
            document["mcp_servers"]["github"]["default_tools_approval_mode"].as_str(),
            Some("prompt")
        );
        assert_eq!(
            document["mcp_servers"]["bicameral"]["tools"]["bicameral.privacy.erase_subject"]
                ["approval_mode"]
                .as_str(),
            Some("prompt")
        );
        assert!(after.starts_with("# operator config\n"));
        assert!(after.contains("[projects.\"/home/op/repo\"]\ntrust_level = \"trusted\""));
        // Exactly one line was added.
        let baseline_lines: Vec<&str> = CODEX_BASELINE.lines().collect();
        let after_lines: Vec<&str> = after.lines().collect();
        assert_eq!(after_lines.len(), baseline_lines.len() + 1);
        for line in baseline_lines {
            assert!(after_lines.contains(&line), "lost line: {line}");
        }
    }

    #[test]
    fn codex_repair_is_idempotent_on_correct_state() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        let written = fs::read_to_string(&p.codex_config).unwrap();

        let second = reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!second.applied);
        assert_eq!(second.plan, PermissionPlan::NoChange);
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), written);
    }

    #[test]
    fn codex_existing_stricter_server_mode_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        let text = CODEX_BASELINE.replace(
            "[mcp_servers.bicameral]\ncommand",
            "[mcp_servers.bicameral]\ndefault_tools_approval_mode = \"prompt\"\ncommand",
        );
        write(&p.codex_config, &text);

        let outcome =
            reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert!(matches!(
            outcome.plan,
            PermissionPlan::StricterPolicyPreserved { .. }
        ));
        assert_eq!(outcome.observation.state, HostPermissionState::Missing);
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), text);
    }

    #[test]
    fn codex_without_registered_server_table_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        let text = "approval_policy = \"on-request\"\n\n[mcp_servers.github]\ncommand = \"x\"\n";
        write(&p.codex_config, text);

        let outcome =
            reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert_eq!(outcome.observation.state, HostPermissionState::Missing);
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), text);
    }

    #[test]
    fn codex_malformed_config_is_never_rewritten() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, "[mcp_servers.bicameral\ncommand = ");

        let outcome =
            reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert!(matches!(outcome.plan, PermissionPlan::Malformed { .. }));
        assert_eq!(outcome.observation.state, HostPermissionState::Drifted);
        assert_eq!(
            fs::read_to_string(&p.codex_config).unwrap(),
            "[mcp_servers.bicameral\ncommand = "
        );
    }

    #[test]
    fn codex_uninstall_restores_only_the_bicameral_owned_key() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();

        let removal = remove(DeveloperHost::Codex, &p).unwrap();

        assert_eq!(removal.removed, vec!["default_tools_approval_mode=approve"]);
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), CODEX_BASELINE);
        assert!(!p.owned_state.exists());
    }

    #[test]
    fn codex_uninstall_restores_prior_operator_value_and_keeps_tightened_value() {
        // Prior value existed as "approve" written by the operator: ours is a
        // no-op grant, so nothing is owned and nothing is restored.
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        let approved = CODEX_BASELINE.replace(
            "[mcp_servers.bicameral]\ncommand",
            "[mcp_servers.bicameral]\ndefault_tools_approval_mode = \"approve\"\ncommand",
        );
        write(&p.codex_config, &approved);
        let outcome =
            reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        assert_eq!(outcome.plan, PermissionPlan::NoChange);
        assert!(outcome.observation.owned_rules.is_empty());
        assert!(remove(DeveloperHost::Codex, &p).unwrap().removed.is_empty());
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), approved);

        // Operator tightens our grant after setup: uninstall keeps their value.
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        let tightened = fs::read_to_string(&p.codex_config).unwrap().replace(
            "default_tools_approval_mode = \"approve\"",
            "default_tools_approval_mode = \"prompt\"",
        );
        write(&p.codex_config, &tightened);
        assert_eq!(
            observe(DeveloperHost::Codex, &p, server_scoped()).state,
            HostPermissionState::Drifted
        );
        let second = reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        assert!(
            !second.applied,
            "repair must not clobber a tightened operator value"
        );

        let removal = remove(DeveloperHost::Codex, &p).unwrap();

        assert!(removal.removed.is_empty());
        assert_eq!(
            removal.preserved_operator_changes,
            vec!["default_tools_approval_mode=approve"]
        );
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), tightened);
    }

    #[test]
    fn codex_writer_refuses_host_authority_keys() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        for rule in [
            "approval_policy=never",
            "sandbox_mode=danger-full-access",
            "network_access=true",
            "tools.github.approval_mode=approve",
            "tools.bicameral.context.approval_mode=deny",
            "default_tools_approval_mode=prompt",
        ] {
            let error = apply_codex_grant(&p.codex_config, &[rule.to_string()]).unwrap_err();
            assert!(error.to_string().contains("refusing"), "{rule}: {error}");
        }
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), CODEX_BASELINE);
    }

    // ── registry fallback ────────────────────────────────────────────────

    fn registry() -> DeclaredToolRegistry {
        DeclaredToolRegistry::parse(
            r#"{"tools": [
                {"name": "bicameral.review.accept_candidate", "action_class": "review_mutation"},
                {"name": "bicameral.lookup", "action_class": "read_only"},
                {"name": "bicameral.context", "action_class": "read_only"},
                {"name": "bicameral.history", "action_class": "read_only"},
                {"name": "bicameral.context", "action_class": "read_only"},
                {"name": "bicameral.workspace.bind", "action_class": "binding_mutation"},
                {"name": "bicameral.governance.resolve", "action_class": "compliance_resolution"},
                {"name": "bicameral.request_correction.approve", "action_class": "read_only"},
                {"name": "bicameral.privacy.erase_subject"},
                {"name": "unrelated.tool", "action_class": "read_only"}
            ]}"#,
        )
    }

    #[test]
    fn fallback_allowlist_is_registry_derived_sorted_and_deterministic() {
        let first = derive_fallback_allowlist(&registry()).unwrap();
        let second = derive_fallback_allowlist(&registry()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            first,
            vec![
                "bicameral.context",
                "bicameral.history",
                "bicameral.lookup",
                "bicameral.request_correction.approve",
            ]
        );
        // Mutation, governance, unclassified, and non-Bicameral tools excluded.
        for excluded in [
            "bicameral.review.accept_candidate",
            "bicameral.workspace.bind",
            "bicameral.governance.resolve",
            "bicameral.privacy.erase_subject",
            "unrelated.tool",
        ] {
            assert!(!first.iter().any(|name| name == excluded), "{excluded}");
        }
    }

    #[test]
    fn fallback_requires_a_classified_registry() {
        let names_only = DeclaredToolRegistry::parse("bicameral.context\nbicameral.lookup\n");
        assert_eq!(names_only.tools.len(), 2);
        assert_eq!(derive_fallback_allowlist(&names_only), None);
        assert_eq!(
            derive_fallback_allowlist(&DeclaredToolRegistry::default()),
            None
        );
    }

    #[test]
    fn claude_fallback_grants_only_derived_per_tool_rules() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        let reg = registry();

        let outcome = reconcile(
            DeveloperHost::Claude,
            &p,
            fallback_only(),
            true,
            Some(&reg),
            NOW,
        )
        .unwrap();

        assert!(outcome.applied);
        assert_eq!(
            outcome.observation.mechanism,
            Some(HostPermissionMechanism::RegistryFallback)
        );
        assert_eq!(outcome.observation.state, HostPermissionState::Configured);
        let allow = claude_json(&p.claude_settings)["permissions"]["allow"].clone();
        assert_eq!(
            allow,
            serde_json::json!([
                "Bash(git status:*)",
                "mcp__github__list_issues",
                "mcp__bicameral__bicameral.context",
                "mcp__bicameral__bicameral.history",
                "mcp__bicameral__bicameral.lookup",
                "mcp__bicameral__bicameral.request_correction.approve"
            ])
        );
        assert!(!allow.to_string().contains("mcp__bicameral\""));
        // Idempotent, then uninstall removes exactly those four.
        let second = reconcile(
            DeveloperHost::Claude,
            &p,
            fallback_only(),
            true,
            Some(&reg),
            NOW,
        )
        .unwrap();
        assert_eq!(second.plan, PermissionPlan::NoChange);
        let removal = remove(DeveloperHost::Claude, &p).unwrap();
        assert_eq!(removal.removed.len(), 4);
        assert_eq!(
            claude_json(&p.claude_settings)["permissions"]["allow"],
            serde_json::json!(["Bash(git status:*)", "mcp__github__list_issues"])
        );
    }

    #[test]
    fn codex_fallback_grants_only_derived_per_tool_approvals() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        let reg = registry();

        let outcome = reconcile(
            DeveloperHost::Codex,
            &p,
            fallback_only(),
            true,
            Some(&reg),
            NOW,
        )
        .unwrap();

        assert!(outcome.applied);
        let document: toml_edit::DocumentMut = fs::read_to_string(&p.codex_config)
            .unwrap()
            .parse()
            .unwrap();
        let table = document["mcp_servers"]["bicameral"].as_table().unwrap();
        assert!(table.get("default_tools_approval_mode").is_none());
        let tools = table["tools"].as_table().unwrap();
        for name in ["bicameral.context", "bicameral.history", "bicameral.lookup"] {
            assert_eq!(
                tools[name]["approval_mode"].as_str(),
                Some("approve"),
                "{name}"
            );
        }
        assert_eq!(
            tools["bicameral.privacy.erase_subject"]["approval_mode"].as_str(),
            Some("prompt")
        );
        assert_eq!(document["approval_policy"].as_str(), Some("on-request"));

        let removal = remove(DeveloperHost::Codex, &p).unwrap();
        assert_eq!(removal.removed.len(), 4);
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), CODEX_BASELINE);
    }

    #[test]
    fn unsupported_host_without_registry_leaves_tools_prompted() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);

        let outcome =
            reconcile(DeveloperHost::Claude, &p, fallback_only(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert!(matches!(outcome.plan, PermissionPlan::Unsupported { .. }));
        assert_eq!(outcome.observation.state, HostPermissionState::Unsupported);
        assert_eq!(
            fs::read_to_string(&p.claude_settings).unwrap(),
            CLAUDE_BASELINE
        );
    }

    // ── readiness boundary ───────────────────────────────────────────────

    #[test]
    fn configured_permission_never_manufactures_activation() {
        use bicameral_api::capability_readiness::{
            CapabilityReadinessState, DeveloperHostId, McpReadinessObservation,
        };
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        assert_eq!(outcome.observation.state, HostPermissionState::Configured);

        let readiness = McpReadinessObservation {
            subject_id: DeveloperHostId::Claude,
            artifact_generation:
                "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            package_installed: true,
            registration_verified: true,
            managed_hook_verified: true,
            missing_prerequisites: Vec::new(),
            drift_reason_codes: Vec::new(),
            activation: None,
            host_permission: Some(outcome.observation),
            observed_at: NOW.into(),
        }
        .reduce()
        .unwrap();

        assert_eq!(
            readiness.state,
            CapabilityReadinessState::InstalledPendingHostAction
        );
        assert_ne!(readiness.state, CapabilityReadinessState::ActiveVerified);
        assert_eq!(
            readiness.host_permission.map(|p| p.state),
            Some(HostPermissionState::Configured)
        );
    }

    // ── review corrections (PR #1247) ────────────────────────────────────

    fn assert_no_backups(root: &Path) {
        fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, found);
                } else {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    if name.ends_with(".bak") || name.contains(".bicameral-tmp-") {
                        found.push(path);
                    }
                }
            }
        }
        let mut found = Vec::new();
        walk(root, &mut found);
        assert!(found.is_empty(), "leftover backup/temp files: {found:?}");
    }

    fn claude_with(allow: &[&str], deny: &[&str], ask: &[&str]) -> String {
        serde_json::json!({
            "permissions": {"allow": allow, "deny": deny, "ask": ask},
            "env": {"KEEP": "1"}
        })
        .to_string()
    }

    #[test]
    fn claude_server_allow_with_covering_deny_is_not_configured() {
        for covering in [CLAUDE_SERVER_RULE, CLAUDE_SERVER_GLOB_RULE, "mcp__*", "*"] {
            let root = tempfile::tempdir().unwrap();
            let p = paths(&root);
            let text = claude_with(&[CLAUDE_SERVER_RULE, "Bash(ls:*)"], &[covering], &[]);
            write(&p.claude_settings, &text);

            let observation = observe(DeveloperHost::Claude, &p, server_scoped());
            assert_eq!(
                observation.state,
                HostPermissionState::Missing,
                "deny {covering}"
            );

            let outcome =
                reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
            assert!(!outcome.applied, "deny {covering}");
            assert!(matches!(
                outcome.plan,
                PermissionPlan::StricterPolicyPreserved { .. }
            ));
            assert_ne!(outcome.observation.state, HostPermissionState::Configured);
            // Stricter policy preserved exactly; nothing removed or weakened.
            assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), text);
            assert!(!p.owned_state.exists());
        }
    }

    #[test]
    fn claude_server_allow_with_covering_ask_is_not_configured() {
        for covering in [CLAUDE_SERVER_RULE, CLAUDE_SERVER_GLOB_RULE, "mcp__*"] {
            let root = tempfile::tempdir().unwrap();
            let p = paths(&root);
            let text = claude_with(&[CLAUDE_SERVER_RULE], &[], &[covering]);
            write(&p.claude_settings, &text);

            let observation = observe(DeveloperHost::Claude, &p, server_scoped());
            assert_eq!(
                observation.state,
                HostPermissionState::Missing,
                "ask {covering}"
            );
            let outcome =
                reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
            assert!(!outcome.applied);
            assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), text);
        }
    }

    #[test]
    fn claude_owned_grant_overridden_by_stricter_operator_policy_is_drifted() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        assert_eq!(
            observe(DeveloperHost::Claude, &p, server_scoped()).state,
            HostPermissionState::Configured
        );
        // Operator later adds a covering deny; our allow rule is still present
        // but no longer effective.
        let mut edited = claude_json(&p.claude_settings);
        edited["permissions"]["deny"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::Value::String("mcp__bicameral__*".into()));
        let text = serde_json::to_string_pretty(&edited).unwrap();
        write(&p.claude_settings, &text);

        let observation = observe(DeveloperHost::Claude, &p, server_scoped());
        assert_eq!(observation.state, HostPermissionState::Drifted);
        assert_eq!(observation.owned_rules, vec![CLAUDE_SERVER_RULE]);

        // Repair preserves the stricter policy and writes nothing.
        let repair =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        assert!(!repair.applied);
        assert!(matches!(
            repair.plan,
            PermissionPlan::StricterPolicyPreserved { .. }
        ));
        assert_eq!(repair.observation.state, HostPermissionState::Drifted);
        assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), text);
        assert_no_backups(root.path());
    }

    #[test]
    fn claude_preexisting_stricter_policy_without_owned_grant_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        let text = claude_with(&["Bash(ls:*)"], &["mcp__bicameral"], &[]);
        write(&p.claude_settings, &text);

        let observation = observe(DeveloperHost::Claude, &p, server_scoped());
        assert_eq!(observation.state, HostPermissionState::Missing);
        assert!(observation.owned_rules.is_empty());
        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        assert!(!outcome.applied);
        assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), text);
        assert!(!p.owned_state.exists());
    }

    // ── malformed ownership state fails closed ───────────────────────────

    #[test]
    fn malformed_ownership_state_blocks_every_host_config_write() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        write(&p.codex_config, CODEX_BASELINE);
        write(&p.owned_state, "{ \"schema_version\": 1, \"hosts\": ");
        let malformed = fs::read(&p.owned_state).unwrap();

        assert!(owned_grant(&p, DeveloperHost::Claude).is_err());
        for host in [DeveloperHost::Claude, DeveloperHost::Codex] {
            let plan = plan(host, &p, server_scoped(), None).unwrap();
            assert!(
                matches!(plan, PermissionPlan::OwnershipStateMalformed { .. }),
                "{host:?}: {plan:?}"
            );
            assert_eq!(
                observe(host, &p, server_scoped()).state,
                HostPermissionState::Drifted
            );
            let outcome = reconcile(host, &p, server_scoped(), true, None, NOW).unwrap();
            assert!(!outcome.applied);
            assert!(matches!(
                outcome.plan,
                PermissionPlan::OwnershipStateMalformed { .. }
            ));
            assert!(remove(host, &p).is_err());
        }
        // Zero host-config writes; the ownership file is preserved verbatim.
        assert_eq!(
            fs::read_to_string(&p.claude_settings).unwrap(),
            CLAUDE_BASELINE
        );
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), CODEX_BASELINE);
        assert_eq!(fs::read(&p.owned_state).unwrap(), malformed);
        assert_no_backups(root.path());
    }

    #[test]
    fn unsupported_ownership_schema_also_blocks_writes() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        write(&p.owned_state, "{\"schema_version\": 9, \"hosts\": {}}");

        let outcome =
            reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();

        assert!(!outcome.applied);
        assert!(matches!(
            outcome.plan,
            PermissionPlan::OwnershipStateMalformed { .. }
        ));
        assert_eq!(
            fs::read_to_string(&p.claude_settings).unwrap(),
            CLAUDE_BASELINE
        );
    }

    // ── uninstall retains ownership for surviving elements ───────────────

    fn owned_rules(p: &HostPermissionPaths, host: DeveloperHost) -> Option<Vec<String>> {
        owned_grant(p, host).unwrap().map(|g| g.rules_added)
    }

    #[test]
    fn uninstall_with_malformed_claude_config_retains_ownership_and_is_repeatable() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        write(&p.claude_settings, "{ broken");

        for _ in 0..2 {
            let removal = remove(DeveloperHost::Claude, &p).unwrap();
            assert!(removal.removed.is_empty());
            assert_eq!(removal.preserved_operator_changes, vec![CLAUDE_SERVER_RULE]);
            assert_eq!(fs::read_to_string(&p.claude_settings).unwrap(), "{ broken");
            assert_eq!(
                owned_rules(&p, DeveloperHost::Claude),
                Some(vec![CLAUDE_SERVER_RULE.to_string()])
            );
        }
        // Once the operator repairs the file, the surviving Bicameral rule is
        // still known and can be removed.
        write(
            &p.claude_settings,
            &claude_with(&["Bash(ls:*)", CLAUDE_SERVER_RULE], &[], &[]),
        );
        let removal = remove(DeveloperHost::Claude, &p).unwrap();
        assert_eq!(removal.removed, vec![CLAUDE_SERVER_RULE]);
        assert_eq!(owned_rules(&p, DeveloperHost::Claude), None);
        assert_eq!(
            claude_json(&p.claude_settings)["permissions"]["allow"],
            serde_json::json!(["Bash(ls:*)"])
        );
        assert_no_backups(root.path());
    }

    #[test]
    fn uninstall_with_malformed_codex_config_retains_ownership_and_is_repeatable() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        write(&p.codex_config, "[mcp_servers.bicameral\n");

        for _ in 0..2 {
            let removal = remove(DeveloperHost::Codex, &p).unwrap();
            assert!(removal.removed.is_empty());
            assert_eq!(
                removal.preserved_operator_changes,
                vec!["default_tools_approval_mode=approve"]
            );
            assert_eq!(
                fs::read_to_string(&p.codex_config).unwrap(),
                "[mcp_servers.bicameral\n"
            );
            let grant = owned_grant(&p, DeveloperHost::Codex).unwrap().unwrap();
            assert_eq!(
                grant.rules_added,
                vec!["default_tools_approval_mode=approve"]
            );
            assert_eq!(grant.prior_value, None);
        }
        assert_no_backups(root.path());
    }

    #[test]
    fn uninstall_after_operator_replaced_claude_rule_drops_ownership_without_clobbering() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        reconcile(DeveloperHost::Claude, &p, server_scoped(), true, None, NOW).unwrap();
        let mut edited = claude_json(&p.claude_settings);
        edited["permissions"]["allow"] = serde_json::json!([
            "Bash(git status:*)",
            "mcp__github__list_issues",
            "mcp__bicameral__bicameral.context"
        ]);
        write(
            &p.claude_settings,
            &serde_json::to_string_pretty(&edited).unwrap(),
        );

        let removal = remove(DeveloperHost::Claude, &p).unwrap();

        assert!(removal.removed.is_empty());
        assert!(removal.preserved_operator_changes.is_empty());
        assert_eq!(removal.absent, vec![CLAUDE_SERVER_RULE]);
        assert_eq!(claude_json(&p.claude_settings), edited);
        // Nothing of Bicameral's survives, so no ownership remains.
        assert_eq!(owned_rules(&p, DeveloperHost::Claude), None);
        let again = remove(DeveloperHost::Claude, &p).unwrap();
        assert!(again.removed.is_empty() && again.preserved_operator_changes.is_empty());
        assert_eq!(claude_json(&p.claude_settings), edited);
    }

    #[test]
    fn uninstall_after_operator_tightened_codex_value_retains_ownership_and_prior() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        let with_prior = CODEX_BASELINE.replace(
            "[mcp_servers.bicameral]\ncommand",
            "[mcp_servers.bicameral]\ndefault_tools_approval_mode = \"auto\"\ncommand",
        );
        write(&p.codex_config, &with_prior);
        // "auto" is the Codex default; an explicit operator value is treated
        // as their policy, so nothing is applied and nothing is owned.
        let first = reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        assert!(!first.applied);

        // Start from the baseline instead, apply, then have the operator tighten.
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        reconcile(DeveloperHost::Codex, &p, server_scoped(), true, None, NOW).unwrap();
        let tightened = fs::read_to_string(&p.codex_config).unwrap().replace(
            "default_tools_approval_mode = \"approve\"",
            "default_tools_approval_mode = \"writes\"",
        );
        write(&p.codex_config, &tightened);

        for _ in 0..2 {
            let removal = remove(DeveloperHost::Codex, &p).unwrap();
            assert!(removal.removed.is_empty());
            assert_eq!(
                removal.preserved_operator_changes,
                vec!["default_tools_approval_mode=approve"]
            );
            assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), tightened);
            assert_eq!(
                owned_rules(&p, DeveloperHost::Codex),
                Some(vec!["default_tools_approval_mode=approve".to_string()])
            );
        }
        assert_eq!(
            observe(DeveloperHost::Codex, &p, server_scoped()).state,
            HostPermissionState::Drifted
        );
        assert_no_backups(root.path());
    }

    #[test]
    fn partial_fallback_rule_removal_keeps_ownership_only_for_preserved_rules() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.codex_config, CODEX_BASELINE);
        let reg = registry();
        reconcile(
            DeveloperHost::Codex,
            &p,
            fallback_only(),
            true,
            Some(&reg),
            NOW,
        )
        .unwrap();
        assert_eq!(owned_rules(&p, DeveloperHost::Codex).unwrap().len(), 4);
        // Operator tightens one of our per-tool approvals and deletes another.
        let mut document: toml_edit::DocumentMut = fs::read_to_string(&p.codex_config)
            .unwrap()
            .parse()
            .unwrap();
        document["mcp_servers"]["bicameral"]["tools"]["bicameral.history"]["approval_mode"] =
            toml_edit::value("prompt");
        document["mcp_servers"]["bicameral"]["tools"]
            .as_table_mut()
            .unwrap()
            .remove("bicameral.lookup");
        write(&p.codex_config, &document.to_string());

        let removal = remove(DeveloperHost::Codex, &p).unwrap();

        assert_eq!(
            removal.removed,
            vec![
                "tools.bicameral.context.approval_mode=approve",
                "tools.bicameral.request_correction.approve.approval_mode=approve",
            ]
        );
        assert_eq!(
            removal.preserved_operator_changes,
            vec!["tools.bicameral.history.approval_mode=approve"]
        );
        assert_eq!(
            removal.absent,
            vec!["tools.bicameral.lookup.approval_mode=approve"]
        );
        assert_eq!(
            owned_rules(&p, DeveloperHost::Codex),
            Some(vec![
                "tools.bicameral.history.approval_mode=approve".to_string()
            ])
        );
        let after: toml_edit::DocumentMut = fs::read_to_string(&p.codex_config)
            .unwrap()
            .parse()
            .unwrap();
        let tools = after["mcp_servers"]["bicameral"]["tools"]
            .as_table()
            .unwrap();
        assert_eq!(
            tools["bicameral.history"]["approval_mode"].as_str(),
            Some("prompt")
        );
        assert_eq!(
            tools["bicameral.privacy.erase_subject"]["approval_mode"].as_str(),
            Some("prompt")
        );
        assert!(tools.get("bicameral.context").is_none());
        assert_eq!(after["approval_policy"].as_str(), Some("on-request"));

        // Repeat uninstall: idempotent, still preserving the operator value.
        let again = remove(DeveloperHost::Codex, &p).unwrap();
        assert!(again.removed.is_empty());
        assert_eq!(
            again.preserved_operator_changes,
            vec!["tools.bicameral.history.approval_mode=approve"]
        );
        assert_eq!(
            owned_rules(&p, DeveloperHost::Codex),
            Some(vec![
                "tools.bicameral.history.approval_mode=approve".to_string()
            ])
        );
        assert_no_backups(root.path());
    }

    #[test]
    fn normal_reconcile_and_uninstall_leave_no_backup_or_temp_files() {
        let root = tempfile::tempdir().unwrap();
        let p = paths(&root);
        write(&p.claude_settings, CLAUDE_BASELINE);
        write(&p.codex_config, CODEX_BASELINE);
        for host in [DeveloperHost::Claude, DeveloperHost::Codex] {
            reconcile(host, &p, server_scoped(), true, None, NOW).unwrap();
            assert_no_backups(root.path());
            reconcile(host, &p, server_scoped(), true, None, NOW).unwrap();
            assert_no_backups(root.path());
            remove(host, &p).unwrap();
            assert_no_backups(root.path());
        }
        assert_eq!(
            fs::read_to_string(&p.claude_settings)
                .unwrap()
                .trim()
                .is_empty(),
            false
        );
        assert_eq!(fs::read_to_string(&p.codex_config).unwrap(), CODEX_BASELINE);
    }
}
