# Bicameral Factory Context

This file separates three context layers that can otherwise get overloaded in agent sessions:

1. **PAMA Context** — the systems-agnostic governance framework for adaptive agent learning and mutation authority. Source: `PAMA.md`.
2. **Factory Context** — this repository's operational language for skills, charters, blueprints, runners, attestations, corrections, and domain-promotion workflows.
3. **Bicameral Context** — the product/domain language maintained in `BicameralAI/bicameral-bot/CONTEXT.md`, used when factory work touches the Bicameral product domain.

Agents should load the smallest context set that fits the task, but must not blur the authority boundaries:

- PAMA terms describe general mutation-governance theory.
- Factory terms describe how Bicameral agents and software-factory runs are governed.
- Bicameral terms describe product concepts such as Decisions, evidence, governance policy, and the Decision Ledger.

## PAMA Context

PAMA, Proportional Adaptive Mutation Authority, is the framework in `PAMA.md`. It governs how adaptive agent systems can learn without turning every observation into hidden authority.

**PAMA**:
The systems-agnostic governance framework for adaptive agent mutation. It says adaptation should be broadly possible, while authority to make a mutation durable, influential, shared, action-enabling, or governance-changing should rise with consequence.
_Avoid_: generic process docs, project management, product ontology

**Mutation Target Class**:
The consequence category of what an adaptive change would modify, from execution-local context through reusable procedure, shared fact, external-action authority, or governance policy. Bicameral factory docs use M1-M5 as lightweight labels for artifact consequence.
_Avoid_: priority, implementation layer, repo folder

**Downstream Authority Class**:
The maximum behavior a mutation may influence, from retrieval-only context through recommendations, drafts, workflow mutation, external action, or governance change. Authority is separate from whether a procedure works.
_Avoid_: confidence score, test result, agent identity

**Adaptive Mutation**:
A proposed change to retained memory, procedure, routing, capability, permission, or governance based on observations, corrections, validated outcomes, or repeated patterns.
_Avoid_: automatic improvement, every edit, product feature

**Promotion Boundary**:
The point where an observation or candidate becomes durable, shared, default, action-shaping, or governance-relevant. PAMA places review at promotion and consequence boundaries, not at every observation boundary.
_Avoid_: every inference, every note, hidden self-edit

**Demotion / Revocation**:
The ability to narrow, supersede, suspend, remove, or block retained adaptive state when corrections, failures, conflicts, stale assumptions, or unsafe outcomes appear.
_Avoid_: cleanup only, blame process, irreversible learning

## Factory Context

Factory context is owned by this repository. It defines the language for Bicameral's software-factory layer: the governed memory boundary for agent procedures, role charters, workflow blueprints, validators, templates, and correction records.

**Software Factory**:
The repeatable production machinery for turning issues or specs into reviewable software changes. It optimizes throughput while leaving product acceptance to existing authority gates.
_Avoid_: agent platform, automation bot, autonomous dev team

**Factory Artifact**:
A tracked repository artifact that can shape future agent behavior, such as a skill, charter, blueprint, template, knowledge note, or correction. Factory artifacts require review because they can influence later work.
_Avoid_: chat note, scratch prompt, local setup

**Product Line**:
A factory-owned context package that names a Bicameral commercial promise, buyer, rollout sequence, OSS/premium boundary, and product prioritization logic. Product lines guide product decisions and factory runs, but they do not replace product-repo ADRs or implementation specs.
_Avoid_: product feature, sales deck only, implementation contract

**Factory Orchestrator**:
The role that turns a structured spec into a planned factory run by preparing context, selecting the configured workspace and worker, invoking checks, and producing PR evidence. It coordinates the run but is not the coding worker or product approver.
_Avoid_: feature agent, reviewer, product owner

**Product Manager**:
The scheduled factory role that turns GitHub issues into governed factory runs by annotating ticket state, resolving dependency order, assigning ready issues to an implementation worker, and integrating eligible completed PRs into the managed branch. It manages intake workflow and surfaces outcomes back to issues; it is not the coding worker, product approver, release owner, or governance maintainer.
_Avoid_: product owner, reviewer, feature agent, release manager

**Managed Branch**:
The integration branch that the Product Manager may advance after governed implementation PRs satisfy the PM merge gate. For Bicameral product repos this is `dev`; `main` remains outside PM merge authority.
_Avoid_: release branch, production branch, product acceptance

**Factory Report**:
A scheduled Product Manager report posted to the configured team channel when a PM cron run has no further GitHub or Devin action to take. It summarizes dependency chains, merged versus human-review leaves, issues needing human input, and factory artifacts that may need demotion.
_Avoid_: run log, release notes, product status report

**Demotion Candidate**:
A factory artifact that issue activity, PR history, attestations, or correction records suggest may need patching, narrowing, flagging, retiring, or conversion into a correction record. Candidate status is advisory evidence for human governance review, not an automatic demotion.
_Avoid_: product bug, release blocker, product acceptance decision

**Agent Task Issue**:
A GitHub issue explicitly labeled for factory intake, initially with `agent-task`, that may be triaged by the Product Manager. The label grants eligibility for ticket management, not automatic implementation.
_Avoid_: approved work, ready issue, product acceptance

**Implementation Job**:
A bounded worker session launched from a ready Agent Task Issue through the governed factory playbook. The job drafts implementation evidence and usually a draft PR, then reports completed or blocked state back to the issue.
_Avoid_: product approval, deployment, merge authority

**Feature Agent**:
The implementation role that drafts scoped product changes from the injected spec and context. Devin and manual collaborator AI sessions are execution surfaces of this role unless their permissions materially differ.
_Avoid_: orchestrator, approver, governance maintainer

**Review Gate Agent**:
An advisory role that inspects run evidence, diff scope, tests, artifact leakage, and risks before a draft PR is opened. It can block orchestration flow but cannot approve product acceptance or merge.
_Avoid_: code owner, approver, CI replacement

**Workspace Substrate**:
The disposable execution environment where the target repo is cloned and the worker runs. The substrate provides isolation and reproducibility; it is not an agent role.
_Avoid_: worker, feature agent

**Worker Profile**:
A configured way to execute the Feature Agent role, including the tool command and permissions for a run. The profile is selected explicitly by the orchestrator and must not switch silently.
_Avoid_: role, authority class

**Context Bundle**:
The ephemeral set of spec, rules, factory context, prompt, plan, and summary files injected into a run. It exists to guide and audit the worker, not to become part of the product diff.
_Avoid_: product docs, committed run record

**Ephemeral Run Root**:
The filesystem location outside the product PR diff where the context bundle, raw logs, and run summary live during a factory run. It may be deleted or archived after evidence is summarized.
_Avoid_: source directory, product artifact folder

**Product PR Artifact**:
A file or change that belongs in the target product repository's pull request because it implements or documents the scoped product work. Factory-run artifacts are excluded unless the target repo requires a factory attestation.
_Avoid_: run bundle, prompt, raw logs

**Factory Attestation**:
A product-repo evidence file that states which factory commit/context materially influenced the PR and whether required factory process steps ran. It is self-attested process evidence, not proof of internal agent cognition.
_Avoid_: signed audit log, product approval

**Domain Entity Candidate**:
A newly observed noun, role, state, artifact, or boundary that may need canonical factory language because it could shape future agent behavior or review decisions. A candidate is not promoted until ownership, evidence, definition, and avoided aliases are reviewed.
_Avoid_: implementation class, one-off label, chat term

**Domain Entity Promotion**:
The review process that turns a domain entity candidate into canonical language in `CONTEXT.md`, or rejects it as product-owned, generic, or an alias. Promotion requires evidence, neighboring-doc checks, and enforcement updates when recurrence matters.
_Avoid_: silent glossary edit, automatic extraction

**Domain Change Graduation**:
The governed workflow for turning a proposed domain-language change into durable factory or product behavior through tracked design, implementation, and validation steps. It prevents glossary updates from outrunning the implementation and validation work needed to make the new language true.
_Avoid_: glossary-only refactor, terminology churn, untracked architecture rewrite

**Context Enforcement Gate**:
A review or CI-adjacent check that forces factory runs and PRs to declare whether new domain entity candidates were introduced and how they were handled. The gate protects `CONTEXT.md` from both drift and over-promotion.
_Avoid_: product approval, type checker

**Boundary Challenge**:
A pre-implementation review step that checks whether a task uses each domain term with the correct authority verb, such as propose, validate, accept, materialize, project, render, or mutate. The challenge turns overloaded inherited terms into explicit human questions before implementation starts.
_Avoid_: general code review, naming cleanup, implementation planning only

**Correction Record**:
A tracked explanation of bad agent output that names the misleading, stale, unsafe, overbroad, or missing factory artifact and the resolution. Corrections feed the demotion loop and may trigger domain entity promotion when missing language caused the failure.
_Avoid_: vague postmortem, private note

**Demotion Loop**:
The correction process that narrows, revises, or retires factory artifacts when they mislead agents or produce bad outputs. It is the counterpart to promoting reusable procedures into shared factory memory.
_Avoid_: postmortem only, blame process

## Bicameral Context

The Bicameral product/domain context is not copied into this factory repository. The source of truth is:

https://github.com/BicameralAI/bicameral-bot/blob/main/CONTEXT.md

Load that file directly when factory work touches Bicameral product behavior, docs, examples, protocol terms, governance policy semantics, or decision-ledger language. Treat it as product/domain context, not factory-governance context.

The following Bicameral product terms are mirrored here because factory domain-promotion gates must recognize them when reviewing Bicameral product PRs. Their product semantics remain owned by `bicameral-bot`.

**validated_sha**:
The durable commit identity anchor recorded on binding evidence after snapshot-scoped verification. It pairs with `graph_snapshot_id` to identify the exact code snapshot whose graph facts were verified; branch names and refs are provenance or resolution inputs only.
_Authority verbs_: record, verify
_Avoid_: branch name, current HEAD, author commit, review approval, confidence score

**GraphScopeCoverage**:
The graph/preflight coverage status that reports whether a graph scope is fully or partially covered for the requested source, diff, binding, or preflight task. It discloses partial indexing honestly so callers do not treat scoped or incomplete local graph evidence as full branch, team, or repository coverage.
_Authority verbs_: report, disclose
_Avoid_: global coverage, team-wide consistency, no-conflict proof, test coverage, confidence score
