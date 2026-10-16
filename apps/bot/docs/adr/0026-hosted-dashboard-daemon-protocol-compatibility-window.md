# ADR-0026: Hosted-Dashboard Daemon Protocol Compatibility Window

**Date:** 2026-07-01
**Status:** accepted
**Level:** L2 cross-repo contract
**Related:** ADR-0020 (gateway authority boundary), ADR-0025 (hosted-tier trust boundary / SOC 2 control scope), ADR-0023 (installation/setup state machine); `docs/specs/cloud-served-dashboard-direct-gateway-data-plane-runtime-architecture.md` (Open Question 4); `docs/specs/cloud-hosted-dashboard-local-daemon-runtime-architecture.md`; BicameralAI/bicameral-bot#697 (product decision), #698 (trust boundary), #711 (implementation)

## Problem

The hosted dashboard SPA is served by Hosted cloud and is **always current**. The
local daemon it renders is **installed on each Member's machine and updates on
that Member's schedule**, so at any moment the cloud-shipped SPA and the local
daemon can be at different protocol versions ("version skew"). This was the
unresolved Open Question 4 in the cloud-served-dashboard runtime spec:

> **Compatibility support window (RFQ):** how many daemon versions the served SPA
> supports before forcing upgrade.

Two failure modes had to be designed out:

- **Silent breakage** — the SPA drives a daemon whose protocol it no longer
  understands, producing confusing partial failures instead of a clear signal.
- **Over-aggressive gating** — forcing an upgrade on any skew at all would make
  the alpha brittle, since minor non-breaking daemon lag is common and harmless.

A related question was **which version** to key compatibility on. Release
version / semver conflates unrelated shipping concerns (packaging, bug-fix
releases, marketing) with wire-protocol compatibility, so it is the wrong key.

## Decision

1. **Key compatibility on `daemon_protocol_version`, not release/semver.** The
   daemon advertises an integer `daemon_protocol_version` (in the pairing
   handshake capability and via a dedicated compatibility endpoint). This integer
   is bumped **only** on a breaking change to the hosted-bridge protocol /
   read-projection contract — never on a release or semver bump. Compatibility
   is decided solely on this integer versus the protocol version the cloud SPA
   expects.

2. **Alpha support window is "current + N-1".** The served SPA supports the
   current protocol version and exactly **one** previous protocol version.
   Rationale (from #697): one previous version absorbs the common case of a
   Member whose daemon auto-update has not yet run, without accumulating an
   unbounded compatibility-shim tail during alpha, when the protocol is still
   moving. The window depth is a single named constant so it can be widened later
   by product decision without code archaeology.

3. **A daemon 2+ breaking versions behind is gated with
   `hosted_dashboard_upgrade_required`.** This is an explicit state (not a
   generic error). The dashboard hides project data and mutable controls and
   shows a **one-action repair path**: either an auto-update button (when the
   daemon supports self-update) or a single copy-paste install/update command.
   The repair carries only a generic command and human copy — never a workspace,
   ledger, source, evidence, or local-path value.

4. **Forward skew degrades safely; it never hard-fails.** A daemon **newer** than
   the cloud SPA expects yields a benign `hosted_dashboard_forward_skew`
   ("dashboard catching up") state: read-only projections still render, mutable
   controls are hidden until the served SPA catches up. This keeps a Member who
   updates their daemon ahead of a cloud deploy from being locked out.

5. **Compatibility is renderer-only readiness metadata (reaffirms ADR-0025).**
   The compatibility check grants no authority and carries no Confidential data.
   The SPA uses it purely to choose which UI state to render; it computes no
   graph/search/freshness/governance authority. The evaluation is mirrored
   client-side so the SPA can render an accurate state (e.g. forward skew) even
   when it cannot reach a compatible daemon.

6. **Telemetry records only non-confidential aggregates.** The daemon keeps an
   in-memory aggregate of the upgrade-required fire rate and the observed
   `daemon_protocol_version` distribution (protocol-version integers and counts
   only). It never records workspace, ledger, source, evidence, provider, or
   local-path data, consistent with the ADR-0025 trust boundary.

## Consequences

- **Positive:** version skew is a designed, observable behavior rather than an
  emergent failure. Members get a clear, one-action recovery when too far behind,
  and are never locked out merely for being ahead. Telemetry gives product a
  signal (fire rate + version distribution) to decide when to widen/narrow the
  window or force an upgrade.
- **Cost:** every breaking hosted-bridge protocol change now requires bumping
  `HOSTED_DASHBOARD_PROTOCOL_VERSION` and keeping the Rust evaluation and its TS
  mirror in lockstep (conformance tests enforce the four cases).
- **Deferred:** auto-update is advertised via a capability flag but the alpha
  ships the copy-paste command path; a true one-click self-update is a daemon
  follow-up. The window depth (N-1) is a product-tunable constant, not a
  permanent guarantee.

## Alternatives considered

- **Key on release version / semver.** Rejected: conflates packaging/marketing
  with wire compatibility and would gate or admit daemons incorrectly.
- **Support only the exact current protocol.** Rejected: too brittle for alpha;
  punishes the common "auto-update hasn't run yet" case.
- **Support an unbounded tail of old protocols.** Rejected: accumulates
  compatibility shims while the protocol is still moving fast in alpha.
- **Hard-fail on forward skew.** Rejected: locks out Members who update ahead of
  a cloud deploy; a read-only catching-up state is strictly safer.
