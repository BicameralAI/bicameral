# ADR-0012: Rust SymbolResolver Boundary and Follow-Ups

**Date:** 2026-06-02  
**Status:** proposed  
**Level:** L1  
**Extends:** ADR-0003 local grounding boundary and ADR-0010 snapshot-scoped graph evidence

The first Rust `SymbolResolver` is a bot-owned local grounding component, not a migration of MCP code-locator authority into `bicameral-bot`. It may carry forward MCP research lessons, especially audited tree-sitter capture kinds and honest evidence states, but compatibility with MCP is through protocol-shaped `GraphSnapshot` and `SymbolOccurrence` evidence, not through MCP-owned persistence, artifact layout, or implementation reuse.

## Decision

`bicameral-bot` owns Rust symbol grounding for the current local repository/worktree boundary. A verified Rust symbol result must be grounded in ADR-0010 evidence semantics: graph snapshot identity, file and line range, content or structural evidence, parser or resolver metadata, and an evidence state that is not approximate, ambiguous, unsupported, stale, or missing.

The Rust resolver should remain Rust-first until each additional language has its own capture-kind audit, corpus strategy, evidence-state expectations, and follow-up issue. TypeScript, Python, Go, Java, or C# support must not be treated as implied by the Rust slice or by MCP prior art.

## Non-Goals

- Do not require MCP walker-retirement shadow mode for this bot-owned Rust slice.
- Do not create MCP-owned local grounding artifacts or persistence in this repo.
- Do not silently accept `BindingEvidence` from ambiguous, unsupported, stale, missing, or approximate symbol results.
- Do not treat search hits or tree-sitter tag matches as verified graph evidence until exact validation has run against the requested snapshot.

## Consequences

The merged #50 implementation is the first implementation slice, not the completed graph-grounding roadmap. Known follow-ups should be explicit work items:

- add real OSS Rust corpus validation that can run deterministically or opt-in without slowing normal CI;
- resolve qualified Rust symbol names beyond file-local symbol names;
- connect verified symbol results into `bind_to_code` materialization once the symbol evidence contract is stable;
- decide whether current-`HEAD` validation is sufficient for local use, or whether historical/non-HEAD `GraphSnapshot` inspection should read blobs from the requested commit;
- decide when reference captures such as calls and implementations become graph edges rather than skipped or advisory data.

These follow-ups are implementation scope, not reasons to blur the ownership boundary. The local daemon remains the authority for verified local grounding evidence, while MCP and other agent surfaces remain clients of that evidence.
