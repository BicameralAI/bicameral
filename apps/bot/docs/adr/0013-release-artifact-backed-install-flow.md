# Release Artifact Backed Install Flow

Bicameral should support a one-line public install through `install.bicameral-ai.com`, but the install script should only bootstrap a signed release binary and dashboard assets and then hand control to `bicameral dashboard`. This keeps the network-facing installer small and stable while putting workspace setup, daemon registration, diagnostics, and future migrations behind versioned CLI/dashboard commands.

> Superseded in part by ADR-0027: bot release artifacts now contain only the
> daemon binary, and Cloud owns SPA serving. The installer integrity and daemon
> setup decisions below otherwise remain applicable.

ADR-0023 owns the install/setup state machine. This ADR owns the release-artifact trust and packaging decision.

## v0 Release Hardening Posture

Decision recorded by @Knapp-Kevin in [#73](https://github.com/BicameralAI/bicameral-bot/issues/73).

### Chosen posture (v0): public artifact origin + SHA256SUMS

Installer-compatible releases publish versioned tarballs, `SHA256SUMS`, and a
short-cache `latest.json` manifest under
`https://pub-7eab94b264754bee8019fd89ca9b5b0b.r2.dev/bicameral`. GitHub Releases remain a release
evidence and mirror location, but the public installer and daemon self-update
path use the public artifact origin so they do not depend on unauthenticated
access to a private repository release endpoint. Verification at install time
consists of:

1. **Manifest resolution** — the install script or daemon updater downloads
   `latest.json`, selects the platform target, and resolves the versioned
   artifact URL.
2. **Checksum verification** — the installer downloads `SHA256SUMS` from the
   same versioned public artifact directory and verifies the tarball digest
   before extraction.

This posture is consistent with the signing stance `bicameral-mcp`'s `SECURITY.md` already advertises (CycloneDX SBOM + Sigstore Rekor). Checksum-only (Option A) was rejected because it would contradict that published stance.

### Next hardening milestone: cosign/minisign detached signatures

Bicameral integrates with numerous external data sources (Jira, Slack, Drive, GitHub, email, transcripts, and others). Organizational and B2B trust will require detached cryptographic signatures (cosign or minisign) before that surface is broadly adopted. This is a committed next milestone, not an optional nice-to-have. Tracked as a follow-up issue from #73.

### macOS notarization: deferred

macOS notarization is not required for the first installer-compatible release. `curl | sh`-installed binaries typically skip Gatekeeper quarantine. The caveat should be documented and notarization named as a planned future step. Revisit when a browser-download or `.pkg` distribution path is offered.

### macOS ad-hoc execution signing

After checksum verification, the installer and daemon self-update path may apply
an ad-hoc signature solely so macOS will execute the local Mach-O. Self-update
must stage the binary, set executable permissions, run
`codesign --force --sign -`, and pass `codesign --verify --strict` before it
replaces the installed binary. Any signing, verification, or replacement
failure preserves the prior binary and leaves sibling assets unchanged.

This local compatibility step does not establish publisher identity and must
not be described as Developer ID signing, notarization, or release
authenticity. SHA-256 verification remains the v0 artifact-integrity gate.

## Consequences

- `install.bicameral-ai.com/install.sh` downloads from the public artifact origin instead of building from source or depending on private GitHub Release endpoints.
- Release assets need stable target names such as `bicameral-aarch64-apple-darwin.tar.gz`, `bicameral-x86_64-apple-darwin.tar.gz`, `bicameral-x86_64-unknown-linux-gnu.tar.gz`, and `bicameral-aarch64-unknown-linux-gnu.tar.gz`, plus `SHA256SUMS`.
- The release workflow publishes tarballs, `SHA256SUMS`, and `latest.json` to the public artifact origin.
- The installer documentation must state exactly what verification is performed (manifest resolution + checksum match), label the trust level as alpha-grade, and name cosign/minisign detached signatures as the planned hardening step.
- The CLI/dashboard need setup, diagnostic, daemon, and service lifecycle commands so stateful setup is handled by Bicameral itself, not by shell script side effects. `bicameral dashboard` is the canonical public first-run command; service installation remains an explicit advanced path.
- `cargo install --git` remains a developer path, not the supported product install path.
