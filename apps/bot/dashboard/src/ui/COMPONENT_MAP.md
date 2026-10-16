# SPA UI component map (T-D / bot#1025 → #1035 → #1092)

**Source of truth for Bot code:** `dashboard/src/ui/*` + tokens in `app.css` / `DESIGN.md`.  
**Production Bot runtime:** `App.svelte` → `HostedDashboard`, entered after a valid Cloud handoff.  
**Production pre-pair owner:** Bicameral Cloud `/dashboard` per Factory #488 Option B light disposition.  
**Bot pre-pair legacy fixture:** `pages/CloudConnectDevice.svelte` / `HostedPairingScreen.svelte`; not production journey authority.

### Alpha product scope (authoritative)

Hosted **Bot `/app` = paired admin flows only** for alpha:

- Products ledger · Create / Setup · Connections · Repair · thin Product admin after Open
- truthful post-pair MCP readiness/setup status

Cloud `/dashboard` owns the complete production device-connection journey before handoff: Cloud auth/access, localhost discovery, pair request/poll, local-confirmation status, typed recovery, and transient handoff. The local daemon remains pairing approval authority.

**Factory #488 ownership disposition:**

- Cloud `/dashboard` = sole production pre-pair renderer/coordinator
- Bot `/app` = post-handoff Products/admin
- daemon = pairing approval authority
- MCP readiness ≠ connection completion and ≠ Products entry gate
- shared machine bytes, if needed, follow the product-repo pin + Factory registry pattern rather than a required Factory-home contract

**Figma authority for pairing:** live Cloud `/dashboard`, subject to the #256 progressive-connect UX correction. The retired four-step install → gateway → pair → MCP rail is not design authority.

**Not in alpha SPA** (lives in **MCP**): Overview, Activity, Decisions, Channels, Journeys.

Canonical Figma references (static journey — language drift noted):

| Surface | Figma node | Production surface |
|---------|------------|--------------------|
| Product ledger default | [113:664](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=113-664) | Operator home / Products |
| Product ledger empty | [114:622](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-622) | Empty products |
| Joining | [114:2090](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2090) | Join in-flight row |
| Join retry | [114:2188](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2188) | Retryable join failure |
| Wrong domain | [114:2287](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=114-2287) | Non-retryable join failure |
| Create Product | [18:77](https://www.figma.com/design/dur4sLDnJSpFThhqKhJlQg/Bicameral-User-Journeys?node-id=18-77) | Hosted create (Product language + Drive gate) |

## Foundations

| Token group | Code | Figma later |
|-------------|------|-------------|
| Surfaces / ink / line | `--canvas`, `--surface`, `--ink-*`, `--line` | Color styles |
| Accent (ops) / Memory | `--accent-*`, `--memory-*` | Color styles |
| Semantic | `--ok-*`, `--warn-*`, `--danger-*` | Color styles |
| Type | `--font-sans`, `--font-mono`, `--text-*` | Text styles |
| Space / radius / shadow | `--sp-*`, `--radius-*`, `--shadow-*` | Variables |

## Primitives (`dashboard/src/ui/`)

| Component | File | Figma component | Variants / notes |
|-----------|------|-----------------|------------------|
| Button | `Button.svelte` | Button | primary, secondary, ghost, danger, dark × sm/md × disabled/loading |
| Input | `Input.svelte` | Input | default, disabled, invalid |
| Textarea | `Textarea.svelte` | Textarea | default, disabled |
| Select | `Select.svelte` | Select | default, disabled |
| FormField | `FormField.svelte` | Form field | label + hint + error |
| Pill | `Pill.svelte` | Status pill | tones + **filled / outline** (ledger uses outline) |
| Dot | `Dot.svelte` | Status dot | tones + optional label |
| Badge | `Badge.svelte` | Hosted / meta badge | teal, purple, neutral |
| Avatar | `Avatar.svelte` | Account avatar | initials from label |
| Card | `Card.svelte` | Card | padding sm/md/lg |
| Callout | `Callout.svelte` | Callout / ! | warn, danger, info, ok + optional action |
| Banner | `Banner.svelte` | Banner | info, warn, danger, ok |
| EmptyState | `EmptyState.svelte` | Empty state | framed (ledger empty card) |
| PageHeader | `PageHeader.svelte` | Page header | eyebrow, title, description, actions |
| GroupHeading | `GroupHeading.svelte` | YOUR PRODUCTS / AVAILABLE FOR… | uppercase section label |
| WorkspaceBar | `WorkspaceBar.svelte` | Workspace topbar | title + WORKSPACE · HOSTED + ⌘K |
| Icon | `Icon.svelte` | Icon set | home, activity, decisions, … |
| NavItem | `NavItem.svelte` | Nav item | default, active, disabled, badge |
| NavSection | `NavSection.svelte` | Nav section | label + items |
| Sidebar | `Sidebar.svelte` | Navigation rail | body + footer slot |
| Topbar | `Topbar.svelte` | Global header | brand, HOSTED, Connected, avatar |
| AppShell | `AppShell.svelte` | App shell | top + sidebar + main |
| SessionBar | `SessionBar.svelte` | CONNECTION block | Connected + SYNC CURRENT |
| Modal | `Modal.svelte` | Modal | title, body, dismiss |
| Tabs | `Tabs.svelte` | Tabs | active tab |
| ConnectionRow | `ConnectionRow.svelte` | Connection row | connected / not_connected / action_needed / unsupported |
| ProductRow | `ProductRow.svelte` | Project/Product row | ready, needs_setup, needs_access, offline, available, joining + join failure |
| DataTable | `DataTable.svelte` | Data table | PRODUCT / EVENT STORE / STATUS / UPDATED |
| Skeleton | `Skeleton.svelte` | Skeleton | width/height |
| Stack | `Stack.svelte` | Stack layout | gap 1–6 |
| Divider | `Divider.svelte` | Divider | plain / labeled |
| Spinner | `Spinner.svelte` | Spinner | size |
| LinkButton | `LinkButton.svelte` | Text link control | — |
| Toast | `Toast.svelte` | Toast | info/ok/warn/danger |
| KeyValue | `KeyValue.svelte` | Detail row | mono value option |
| StepList | `StepList.svelte` | Setup lifecycle panel | valid for Product/setup flows; not the production device-connection wizard |
| Metric | `Metric.svelte` | Stat card | label + value + hint |
| InlineAlert | `InlineAlert.svelte` | Row-level join failure | role=alert |
| InlineCode | `InlineCode.svelte` | Inline code | — |
| GoogleDriveOAuthFlow | `GoogleDriveOAuthFlow.svelte` | Drive OAuth card | idle → starting → authorize → waiting → connected / denied / expired |
| McpReadinessCard / CapabilityStatusSection | readiness components | MCP readiness | post-pair read-only readiness + terminal setup command; never Products entry authority |

## Production hosted routes (`Cloud /dashboard` → handoff → Bot `App` / `HostedDashboard`)

| Route / gate | Purpose | Key components |
|--------------|---------|----------------|
| Cloud `/dashboard` | Sole production device connection / pairing / recovery | Cloud-owned renderer + hosted-bridge pairing contract |
| Bot `/app` Products | Post-handoff operator home / ledger | OperatorHome + post-pair `CapabilityStatusSection` |
| create | Connection-gated create | FormField, Input, Callout, StepList, Button dark |
| setup / registration | Post-create setup | KeyValue, StepList, Callout |
| connections | Provider connections | `GoogleDriveOAuthFlow`, ConnectionRow |
| repair | Repair recovery | KeyValue, Callout, Button |
| product | Thin admin card after Open | Banner, KeyValue, Pill, Button |

### Legacy / eval-only connection components

`CloudConnectDevice.svelte` and `HostedPairingScreen.svelte` may remain temporarily for tests, fixture comparison, or migration support, but they are not production pre-pair presentation authority under Factory #488 Option B. No production route should send an unpaired user through their install → gateway → pair → MCP lifecycle.

## Spec status (honest)

| Flow | Spec? | Production |
|------|-------|------------|
| Hosted device pairing | **Yes** — hosted-bridge protocol + Factory #488 ownership disposition | Cloud `/dashboard` before Bot handoff |
| Google Drive connect | **Yes** — `docs/specs/oauth-and-integration-connections.md` (local_loopback OAuth) | `GoogleDriveOAuthFlow` on Connections + Create gate |
| MCP readiness/setup | **Yes, read-only UI** — `docs/specs/mcp-distribution-and-developer-host-setup.md`; setup remains terminal-only (`bicameral setup mcp`) | Paired Bot `/app` home; never device-connection completion |

**Retired:** `?eval=spa-mock` harness (`dashboard/src/spa-mock/*`) after production promote (#1035).

## Flows to exercise on the real hosted runtime

1. Cloud `/dashboard`: signed in → detect daemon → pair/local approval → transient handoff, with no MCP gate.  
2. Bot `/app`: paired handoff → Products immediately; MCP readiness/setup visible separately.  
3. Empty products → Create product.  
4. Create gated → Connections (GDrive) → Create enabled → Setup → Ready → Open.  
5. Direct/unpaired `/app` navigation → return to Cloud `/dashboard`, never render the retired four-step Bot lifecycle.
