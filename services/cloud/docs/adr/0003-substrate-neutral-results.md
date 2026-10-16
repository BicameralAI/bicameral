# ADR-0003: Substrate-Neutral Results

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1

## Problem

Different workspaces enforce governance through different substrates: git PRs, dashboard warnings, agent warnings, queued actions, or paused approvals. Cloud must not assume every workspace can or should fail CI.

## Decision

Cloud returns substrate-neutral objects such as `ConflictSignal`, `CodeGraphEvidence`, `GroundingSuggestion`, `DependencyRisk`, and advisory `GovernanceResult`. The local bot maps those results to the selected workspace capability.

## Consequences

The same cloud result can become a PR warning in a git workspace, a dashboard flag in another workspace, or an agent warning in a local-only setup.
