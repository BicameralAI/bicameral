# ADR-0001: Cloud Oracle, Not Authority

**Date:** 2026-05-27  
**Status:** proposed  
**Level:** L1

## Problem

The paid product should not be framed as “hosted daemon.” Hosting the control plane would weaken the local-first trust boundary and confuse paid optimization with canonical authority.

## Decision

Cloud is an oracle/optimization layer. It returns evidence, signals, suggestions, and advisory governance results to the local bot. The bot governance policy and event store adapters decide what becomes canonical or enforceable.

## Non-Goals

Cloud does not own local review UX, storage materialization, signoff approval, or source-specific adapter behavior by default.

## Consequences

Paid value concentrates in computation, scale, and precision while free/public Bicameral remains locally useful and code-grounded.
