# ADR-0002: Hosted Code Graph

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1

## Problem

Local grounding is enough for a current worktree, but teams eventually need cross-branch, cross-repo, historical, and dependency-aware reasoning that is too expensive to repeatedly compute locally.

## Decision

Cloud owns the organization-scale hosted code graph: branches, worktrees, repos, symbols, dependencies, deploy/release surfaces, history, and cached expensive indexes. The graph powers conflict prediction, blast-radius analysis, and higher-precision grounding suggestions.

## Non-Goals

The hosted graph is not an event store substrate and not the Decision Ledger.

## Consequences

Cloud can detect risks that local bot cannot honestly claim, while preserving local governance as the authority boundary.
