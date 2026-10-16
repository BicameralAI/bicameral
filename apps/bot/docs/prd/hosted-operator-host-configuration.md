# PRD: Hosted operator host configuration (`/app`)

**Status:** accepted product intent (IA + taxonomy; slices may land incrementally)  
**Owner:** product (Jin)  
**Surfaces:** Bot `/app` post-handoff admin shell (operator-level)  
**Related PRD:** `docs/prd/hosted-device-pairing-bootstrap.md` (`/connect` only — browser↔daemon pair)  
**Related product areas:** event-store substrate · MCP / agent-host setup · future reasoning projection · optional external OAuth catalog  
**Related implementation (illustrative, non-binding):** Bot hosted dashboard SPA · Drive OAuth · MCP readiness projection · operator rail / Settings

---

## Problem statement

After a browser is paired to the local daemon, operators still need to **configure and monitor host-side capabilities** that product work depends on or optionally uses: where durable Product state lives, whether an agent host can call Bicameral via MCP, and later whether reasoning or other projections are available.

Those concerns have been mixed into the wrong UI shapes:

| Anti-pattern | What went wrong |
|---|---|
| **MCP essay on Products home** | Treats developer-host setup as everyday product UI; confuses “browser connected” with “agent host ready”; steals primary scan path |
| **MCP as Products / connect gate** | Implies optional agent-host setup is required for product entry; false coupling |
| **Single “Connections” hub for everything** | Collapses system substrate (e.g. Drive as event store), terminal-driven host setup (MCP), and future third-party OAuth into one marketplace metaphor |
| **Install curriculum on every visit** | Re-teaches architecture; belongs on rare bootstrap (`/connect` help) or dedicated setup, not permanent home chrome |

Operators need a **stable post-pair model** that:

1. Keeps **Products** as the work surface.  
2. Gives **system host configuration** a durable home that can grow.  
3. Shows **health at a glance** without forcing setup pedagogy on every load.  
4. Leaves room for **new host capabilities** and a separate **external integrations** story without another IA rewrite.

---

## Product intent (authoritative)

> **After pair, `/app` is product work by default.**  
> **Host configuration is a first-class operator surface under Settings (system), not a second connect wizard and not a permanent home banner.**  
> **Glanceable host status lives in a persistent strip (rail footer), not in multi-paragraph home content.**  
> **Capability classes stay separated:** system substrate / host services vs external app OAuth.  
> **The dashboard may project status and guide terminal setup; it does not claim to complete host trust, install packages, or replace the daemon.**

This PRD owns **post-pair operator host configuration IA**.  
It does **not** redefine `/connect` (see pairing bootstrap PRD).

---

## Capability taxonomy (extension spine)

All post-pair configuration items belong to one of these classes. New features must pick a class before getting a UI slot.

### Class A — System storage / event substrate

**What:** Where Product decision history and related durable operator state are stored for this host/session model.  
**Alpha:** Google Drive as event store (OAuth is the *mechanism*; the product concept is **storage / event store**).  
**May gate:** product **create** (or other substrate-required actions) when the chosen substrate is unavailable.  
**Must not be labeled as:** “just another marketplace connection” peer to Linear/GitHub when it is the Product event store.

**Extension:** additional substrates (git, other providers, folder modes) appear as storage options or substrate rows — not as generic “apps.”

### Class B — Host services (agent / projection / local tooling)

**What:** Capabilities that run on or through the **local Bicameral host** for this operator, often **terminal-driven** or daemon-mediated.  
**Today:** MCP / developer agent-host readiness (install, trust, activation receipt).  
**Later (in-scope for this taxonomy, not required for v1 UI):** reasoning projection, other daemon-side projections, doctor/repair surfaces that are host-level not product-level.

**Rules:**

| Rule | Meaning |
|---|---|
| Optional unless product explicitly depends on it | MCP must not gate Products list or browser pair completion |
| Dashboard is projection + guidance | Status, reason codes, copy setup command, refresh; no fake “Install” that finishes host trust in-browser |
| Distinct from browser pair | Pair proves browser↔daemon; host services prove something else (e.g. agent host package + trust) |

### Class C — External product integrations (OAuth / API)

**What:** Optional third-party apps that are **not** the event-store substrate and **not** local host services (e.g. GitHub, Linear as product integrations).  
**Auth model:** typically browser OAuth or API keys; connect / disconnect / reauth.  
**IA:** may later earn a **Connections** (or Integrations) catalog — **Settings subsection first**, rail peer only if volume justifies it.

**Must not absorb:** Class A storage framing or Class B MCP/projection setup.

### Class D — Account / product / policy settings (out of primary scope)

User profile, product membership, governance policy UIs, per-Product project settings remain **other surfaces**. This PRD does not merge them into host configuration unless a future decision explicitly re-homes them.

---

## Surface model on `/app`

```text
/app (paired)
├── Products                    ← primary work (list, open, create*, join, repair)
├── Settings                    ← operator host configuration (Class A + B; extensible)
│   ├── Storage                 ← event substrate (Drive alpha)
│   ├── Agent host              ← MCP readiness / setup guidance
│   └── (future sections)       ← e.g. Reasoning projection, other host services
├── Connections (later only)    ← Class C external OAuth catalog when real
└── Host status strip (always)  ← compact Drive + MCP (+ future) → deep-link Settings
```

\*Create may require Class A substrate readiness; that is storage policy, not MCP policy.

### Products home

**Is for:** product inventory and product actions.  
**Is not for:** permanent MCP install essays, multi-step host curriculum, or standing “developer host readiness” cards.

Optional soft discovery (dismissible, not permanent) is allowed only if status-strip + Settings prove insufficient; default is **no home essay**.

### Settings (system host configuration)

**Is for:** Class A and Class B configuration and repair.  
**Naming:** **Settings** (or equivalent “configure this host”), not “Connections,” so OAuth-marketplace expectations do not own system setup.

**Section pattern (stable for extension):**

```text
Settings
  <Section title>
    One-line purpose (system role)
    Status + primary actions (connect / manage / copy command / refresh)
    Detail (account label, reasons, last verified) progressive disclosure
```

New Class B services add **sections** (or cards in a host-services list), not new top-level product modes, until volume forces IA review.

### Connections / Integrations (deferred catalog)

**When:** ≥1 real Class C integration beyond placeholders, or clear discovery need.  
**Where first:** prefer **Settings → Connections** (catalog nested under settings) before a top-level rail item.  
**Never:** put Class A event store and Class B MCP as peer rows in a greyscale “logo wall” of unsupported apps.

### Host status strip (rail footer or equivalent)

**Is for:** always-on, short-label health of Class A (and Class B when relevant).  
**Pattern:** status bar / workspace indicator (short text + semantic color; click → Settings section).  
**Is not for:** reason-code dumps, setup commands, or policy disclaimers.

**Label guidance (examples; exact copy may evolve):**

| Concern | Healthy | Attention | Absent / optional |
|---|---|---|---|
| Storage (Drive alpha) | Drive connected | — | Drive not connected |
| MCP | MCP ready | MCP needs host / needs repair | MCP not set up |

Future host services add **rows or a summarized “Host attention (n)”** chip that expands — same strip, no home banner.

---

## Principles (durable)

1. **Pair ≠ host-complete.** Browser handoff does not imply MCP or projection readiness.  
2. **Work surface purity.** Products stays product-shaped.  
3. **Taxonomy before chrome.** New capability → Class A/B/C → then Settings section, Connections catalog, or elsewhere.  
4. **Status vs setup.** Strip = glance; Settings = act and understand.  
5. **Honest authority.** UI cannot finish terminal host trust or invent substrate health.  
6. **Progressive disclosure.** Detail and terminal commands live in Settings, not on home.  
7. **Gate only true dependencies.** Storage may gate create; MCP must not gate Products entry.  
8. **Extend by section, not by re-platforming.** Prefer new Settings sections over new permanent home modules.  
9. **External OAuth is optional product glue; storage is system.** Different language, different IA.  
10. **Cross-PRD boundary.** Pre-pair pedagogy stays on `/connect` (Show help); post-pair host config stays here.

---

## Relationship to pairing bootstrap PRD

| Concern | Owner PRD |
|---|---|
| `/connect` path, pair flow, install/start Show help, no `/dashboard` product URL | Hosted device pairing bootstrap |
| Session rarity / restore for pair | Pairing bootstrap (follow-on) |
| Post-pair Products vs Settings vs status strip | **This PRD** |
| MCP not a connect step; not pair completion | Both (pairing: not on `/connect`; this PRD: Settings + strip, non-gating) |
| Drive as create substrate | This PRD (Class A); product-create flows obey substrate readiness |

Amendments to path names or pair protocol go to the pairing PRD.  
Amendments to host IA, capability classes, or Settings sections go here.

---

## v1 slice (current product cut)

**In:**

- Products home without permanent MCP readiness essay.  
- Operator **Settings** with at least **Storage** (Drive) and **Agent host** (MCP status + terminal setup guidance + refresh).  
- Host status strip: Drive + MCP short labels; click opens Settings section.  
- No top-level Connections while only Class A OAuth is real.  
- MCP non-gating for Products list / product entry.  
- Create still respects event-substrate readiness (Class A).

**Out of v1 UI (taxonomy reserved):**

- Reasoning / other projection Settings section (may be omitted entirely until real).  
- Class C Connections catalog (GitHub/Linear live connect).  
- Dashboard-performed MCP install/repair.  
- Merging Project Settings / account settings into this surface.

**Implementation may land incrementally** as long as taxonomy and surface rules are not violated.

---

## User stories (representative, extensible)

1. As an operator, I want Products to show my products without setup essays, so I can work immediately after pair.  
2. As an operator, I want to see whether storage is connected at a glance, so I know if I can create a Product.  
3. As an operator, I want to open Settings → Storage to connect or manage Drive, so event-store setup has a clear home.  
4. As an operator, I want to see MCP status without it blocking Products, so optional agent-host setup does not feel mandatory.  
5. As an operator, I want Settings → Agent host to show MCP state and a copyable setup command, so I can finish setup in the terminal.  
6. As an operator, I want clicking a status strip row to jump to the right Settings section, so diagnosis is one step.  
7. As an operator, I want future projection setup to appear as another Settings section, so I do not relearn navigation.  
8. As an operator, I want external app OAuth (when shipped) not mixed with event-store storage language, so I understand system vs optional tools.  
9. As an operator, I want broken host state explained in Settings with refresh, so I can re-check after terminal repair.  
10. As an operator returning daily, I want host status quiet when healthy, so chrome stays calm.

---

## Non-goals

- Redefining hosted bridge pairing protocol or `/connect` UI.  
- Making Cloud install or trust MCP for the operator’s machine.  
- Requiring MCP for Product membership, join, or ledger read when substrate allows.  
- A permanent multi-step “setup wizard” as the default `/app` experience.  
- Specifying every future substrate or projection’s exact control schema (those get their own specs; they **slot into this IA**).

---

## Success criteria

| Signal | Target |
|---|---|
| Mental model | Operators can answer: “Where do I work?” → Products; “Where do I configure host?” → Settings; “Is host OK?” → status strip |
| Extension | A new Class B capability ships as a Settings section (+ optional strip row) without inventing a third primary nav paradigm |
| Separation | Class A vs Class B vs Class C are not peer rows in one undifferentiated connections list |
| Calm home | Healthy host → no setup essay on Products |
| Honest gating | Only true dependencies (e.g. storage for create) block actions |

---

## Decision log

| Date | Decision |
|---|---|
| 2026-08-12 | MCP readiness is post-pair support state; does not mean browser connected; does not gate Products. |
| 2026-08-13 | Prefer dual host status in rail footer (with Drive) over top-of-home MCP card. |
| 2026-08-13 | System host config lives under **Settings** (Storage + Agent host); not a Connections marketplace. |
| 2026-08-13 | Drive is framed as **Storage / event store** (Class A); OAuth is mechanism. |
| 2026-08-13 | Do not keep top-level Connections while only Drive OAuth is real; Class C catalog is later. |
| 2026-08-13 | **Separate PRD** from pairing bootstrap; taxonomy written for extension (projection, more substrates, external OAuth). |

---

## Open follow-ons (do not block this PRD)

- Soft, dismissible “needs setup” link on Products if discovery metrics show Settings is hard to find.  
- Status strip summary chip when many host services exist.  
- Exact copy system and empty states per section.  
- Whether Class C first appears as Settings → Connections vs rail peer.  
- Cross-linking from create-disabled states to Settings → Storage (copy only).  
- Spec/ADR updates when projection or multi-substrate storage ship.

---

## Glossary (product language)

| Term | Meaning in this PRD |
|---|---|
| **Host** | Local Bicameral daemon / operator machine context after pair |
| **Storage / event store** | Class A substrate for Product durability (alpha: Drive) |
| **Agent host** | Class B path for agent tools (MCP) against Bicameral |
| **Host status strip** | Compact always-on indicators → Settings |
| **Settings** | Operator system configuration surface for Class A/B (and nested Class C later) |
| **Connections** | Future Class C external integration catalog (name reserved; not v1 rail) |
| **Pair / connect device** | Browser↔daemon link — pairing bootstrap PRD only |
