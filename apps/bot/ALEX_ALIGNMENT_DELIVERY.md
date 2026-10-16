# Alex Alignment Delivery

This file maps Alex-aligned core product requirements to implementation issues in `bicameral-bot`.

The bot owns the core decision authority layer. Customer-specific workflow expression belongs in `bicameral-sidecar`. Stable shared contracts belong in `bicameral-sdk`.

## Core Delivery Issues

| Product Primitive | Issue | Status | Notes |
|---|---|---|---|
| Reviewed Decision Promotion | #481 | Planned | Raw evidence -> candidate decision -> reviewed decision -> canonical constraint. |
| Constraint Graph and Catalog Read Model | #482 | Planned | Canonical constraint storage, attribution, versioning, evidence links, and affected surfaces. |
| Pre-Flight Context Resolver | #483 | Planned | Relevant decisions and constraints before ticket, branch, PR, or agent session work begins. |
| Contradiction and Drift Detection | #484 | Planned | Reviewable findings with severity, evidence, affected surface, and suggested resolution. |
| Semantic Constraint Mapping | #485 | Planned | Durable mapping from product/customer language into implementation-impacting candidate constraints. |

## Boundary

Core bot owns:

- Authority enforcement
- Candidate extraction and promotion
- Canonical decision and constraint state
- Constraint graph read models
- Pre-flight relevance resolution
- Contradiction and drift detection
- Durable semantic mapping primitives

Core bot does not own:

- Customer-specific approval chains
- Customer-specific dashboard layout
- Static markdown sprint source-of-truth formats
- Customer-specific monitored source systems
- Sidecar workflow packaging
