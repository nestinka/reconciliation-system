# Prompt — Build the Implementation Bundle for a Multi-Tenant Reconciliation Platform

> Paste this into Claude Code (or another AI coding agent) at the root of an empty repository. The agent will produce a complete artefact bundle that a solo developer can then execute against, task by task.

---

## 1. Role

You are a **Senior Technical Lead, Solution Architect, and Compliance-Aware Engineer**. Your job is to produce the complete implementation bundle for a new, standalone, multi-tenant reconciliation platform. The bundle will be executed by a **solo developer working with Claude Code as the primary build agent** — there is no engineering team. Every artefact you produce must be unambiguous enough to be acted on by an AI coding agent without further clarification, and every task must be verifiable.

You make opinionated, defensible choices. You justify every library, framework, and architectural decision with the alternatives considered and the reason for the pick. "Industry standard" is not a reason.

---

## 2. The System

A multi-tenant SaaS platform that performs reconciliation across **three source types**:

1. **Bank statements** — external, batch-oriented. Must support at minimum MT940, BAI2, CAMT.053, CSV, and PDF-extracted data. Ingestion via file upload, SFTP, and operator-triggered fetch.
2. **Internal ledger** — the customer's own system of record. Event-driven, near-real-time. Ingestion via API and webhook.
3. **Cross-system events** — third-party connected systems (payment processors, ERPs, exchanges, custodians, etc.). Ingestion via API, webhook, SFTP, or scheduled polling.

Core capabilities the platform must deliver:

- **Ingestion** with per-source connectors and a pluggable connector interface
- **Normalisation** of every source into a canonical transaction model
- **Matching engine** — rule-based and configurable fuzzy matching, deterministic and replayable
- **Exception management** — unmatched, partial, duplicate, break, ageing
- **Investigation workflow** — assignment, comments, manual matching, write-off, four-eyes approval
- **Audit trail** — immutable, regulator-grade, tamper-evident
- **Reporting and dashboards** — reconciliation status, break analysis, SLA tracking, tenant-level KPIs
- **Tenant administration** — provisioning, RBAC, per-tenant configuration
- **API and webhook surface** — for programmatic ingestion and downstream consumption

---

## 3. Hard Constraints

- **Backend: Rust.** Choose, justify, and lock in the web framework and core crates (HTTP, async runtime, DB client, migrations, validation, serialization, auth, background jobs, file parsing for bank formats, observability). Identify the matching-engine boundary and explain where Rust earns its place vs. where it's overkill.
- **Frontend: Next.js.** Choose, justify, and lock in App Router vs. Pages, rendering strategy (RSC / SSR / CSR / hybrid), UI library / design system, and state model. The frontend is a thin client over a documented HTTP API.
- **Independence.** Frontend and backend must be independently deployable, independently versioned, and not coupled at build time. No shared types via codegen unless explicitly justified — and if codegen is used, the contract source is the backend.
- **Multi-tenancy from day one.** Decide between shared-schema-with-tenant-id, schema-per-tenant, or database-per-tenant. Justify against the compliance requirements below. Tenant isolation must be verifiable through tests.
- **Compliance scope: ISO 27001, FCA, SOC 2** (Security, Availability, Processing Integrity, Confidentiality at minimum). Compliance is designed in, not bolted on. Every phase lands a subset of controls.
- **Production-grade design system from the first commit.** No placeholder UI, no "we'll polish later" — tokens, primitives, components, accessibility baseline (WCAG 2.2 AA target), and dark mode policy decided in Phase 0.
- **Execution model: solo developer + Claude Code.** Task granularity must reflect this. No "stand up a team" steps. No calendar-based estimates — use relative effort only (S / M / L / XL).
- **Scale targets are unspecified.** Design for three tiers, gate scope changes between them, and document explicitly what changes at each tier:
  - **Launch** — low volume, single region, prove the workflow end-to-end
  - **Growth** — order-of-magnitude higher volume, SLA-bound
  - **Scale** — high-throughput matching, multi-region capable, regulator-audited

---

## 4. Output — The Artefact Bundle

Produce **exactly** the following set of files. Do not collapse into a single document. Do not skip files.

```
/CLAUDE.md
/README.md
/docs/
  README.md
  architecture.md
  frontend.md
  backend.md
  database-and-infrastructure.md
  security-and-compliance.md
  testing-strategy.md
  observability-and-operations.md
  development-workflow.md
  implementation-plan.md
  todo/
    PHASE-0-foundations.md
    PHASE-1-...md
    PHASE-N-...md
.claude/
  agents/
    recon-architect.md
    recon-dev.md
    recon-reviewer.md
    recon-security.md
    recon-researcher.md
  skills/
    reconciliation-matching-engine.skill
    multi-tenant-rust-backend.skill
    nextjs-design-system.skill
    compliance-control-mapping.skill
    bank-statement-parsers.skill
```

### File-by-file requirements

**`CLAUDE.md`** — operating manual for any Claude Code session on this repo. Must include: one-paragraph project mission; tech-stack table with versions and why-this-choice; canonical directory layout; commands the agent is allowed to run (build, test, lint, migrate, run-dev); commands the agent must never run unattended (production deploy, destructive migrations, secret rotation); coding standards (Rust edition, clippy level, rustfmt, ESLint / Prettier config, naming, error handling, logging conventions); commit and branching conventions; pickup protocol (read `docs/implementation-plan.md`, find the next unchecked task in the active phase's TODO file, then proceed); explicit guidance on when to invoke each subagent.

**`README.md`** — public-facing. Product summary, feature list, architecture diagram (ASCII or text-based), prerequisites, local dev quickstart, deploy summary, licence, contribution model.

**`docs/architecture.md`** — system context diagram, component diagram, end-to-end data flow for each of the three source types (ingest → normalise → match → expose), trust boundaries, deployment topology for each scale tier, technology choices table with justifications, explicit rejected alternatives, the canonical transaction model at field level, the tenant isolation model, the identity and access model. All diagrams in text/ASCII or Mermaid.

**`docs/frontend.md`** — Next.js setup decisions, design system foundations (typography scale, colour tokens, spacing scale, radius scale, motion policy, component primitives), component library choice, accessibility baseline, i18n strategy, auth integration on the client, API client pattern, error and loading state conventions, form-handling strategy, directory structure, recommended libraries with versions and reasons, build and deploy pipeline.

**`docs/backend.md`** — Rust crate choices with versions and reasons; clean architecture layering (domain / application / infrastructure / interface); module boundaries; the error model; configuration and secrets handling; the matching engine as a separately testable component with a defined input/output contract; API design conventions (REST vs. GraphQL vs. RPC — pick and justify); idempotency, retries, and concurrency control; rate limiting; directory structure; a public API surface sketch for the V1 endpoints.

**`docs/database-and-infrastructure.md`** — database engine choice and version, why, alternatives rejected; multi-tenant data model with the chosen isolation approach; migration strategy and tooling; backup and point-in-time recovery; retention and archival policy (mapped against FCA record-keeping requirements); infrastructure choice (cloud provider or provider-agnostic) with reasoning; IaC tool; environments (dev / staging / prod) and how they differ; network architecture; secrets management.

**`docs/security-and-compliance.md`** — three control-mapping tables:
- ISO 27001 Annex A control → how the system addresses it → which phase lands it
- SOC 2 trust services criterion → control implementation → which phase lands it
- FCA-relevant areas (record-keeping retention, operational resilience, audit trail integrity, customer data handling) → implementation → which phase lands it

Plus: a STRIDE-style threat model summary; data classification scheme; encryption at rest and in transit; key management; authentication factors and session handling; authorisation model (RBAC and/or ABAC); audit logging requirements (separate pipeline from application logs); incident response outline; vendor and supply-chain review checklist.

**`docs/testing-strategy.md`** — testing pyramid for both stacks; unit, integration, contract, end-to-end; property-based testing for the matching engine specifically; mutation testing target; coverage targets per layer; fixture and synthetic-data strategy (especially for the bank-statement formats); CI gates; security testing (SAST, DAST, dependency scanning, secret scanning, SBOM); load and soak testing approach per scale tier; chaos / fault-injection plan for Growth tier onwards.

**`docs/observability-and-operations.md`** — logging standard (structured, correlation IDs, PII redaction); metrics (RED for services, USE for resources); distributed tracing; SLIs and SLOs per scale tier; alerting policy; on-call runbook outline; error budgets; the dashboards to build; the audit-log pipeline (separate from application telemetry); feature-flag system; deployment strategy (blue/green, canary, or both); rollback procedure; disaster recovery RTO / RPO targets per tier.

**`docs/development-workflow.md`** — how a solo developer + Claude Code actually operates: branch model, PR-to-self review pattern using the `recon-reviewer` subagent, when to invoke `recon-security` (mandatory triggers, e.g. any change touching auth, audit log, encryption, tenant isolation, or external connectors), when to call `recon-researcher`, definition of done, and the gate criteria for moving from one phase to the next.

**`docs/implementation-plan.md`** — phased plan. Each phase has:
- Objective
- Scope in
- Scope out
- Prerequisites (other phases or external items)
- Exit criteria (testable, not aspirational)
- Risks and mitigations
- Estimated relative effort (S / M / L / XL — no calendar dates)
- The specific compliance controls landed in this phase
- What is deferred and why

**Minimum phase set:**
- **Phase 0 — Foundations.** Repo, CI/CD, design-system shell, auth, tenant model skeleton, observability skeleton, audit-log pipeline skeleton, baseline security controls. No business logic yet.
- **Phase 1 — Canonical model + first source.** Canonical transaction model implemented; one source connector (bank statements, MT940 or CAMT.053) ingesting end-to-end with normalisation.
- **Phase 2 — Internal ledger source + matching engine v1.** Rule-based matching, deterministic, replayable.
- **Phase 3 — Exception management and investigation workflow.** Including four-eyes approval.
- **Phase 4 — Cross-system events source + connector framework generalisation.**
- **Phase 5 — Reporting, dashboards, tenant admin.**
- **Phase 6 — Hardening, performance, compliance audit-readiness, GA.**

Adjust phase boundaries if you find a better split, but never collapse compliance into a final phase.

**`docs/todo/PHASE-N-*.md`** — per-phase TODO. Every task is a checkbox item with:
- A stable task ID using the format `REC-P{phase}-{nnn}` (e.g. `REC-P0-001`)
- A one-line title
- A body specifying:
  - Acceptance criteria (testable, plural)
  - Files touched (explicit paths)
  - Tests required (which type, which assertions)
  - Observability hooks required (logs, metrics, traces, audit events)
  - Security considerations if any
  - Dependencies on other task IDs
- A **Verification** sub-section listing the exact commands the agent runs to confirm done (e.g. `cargo test --package matching-engine`, `pnpm test --filter web -- --coverage`, `cargo audit`, etc.)

**`docs/README.md`** — index linking all artefacts with a short sentence on what each one is for and the recommended reading order.

**`.claude/agents/*.md`** — subagent definitions. Each must specify model tier, permissions / mode (e.g. read-only, plan-only, accept-edits), responsibilities, refusals, and example invocations.
- **`recon-architect`** — architectural decisions and trade-off analysis. Plan-mode by default.
- **`recon-dev`** — primary build agent. Accept-edits.
- **`recon-reviewer`** — code reviewer. Read-only, plan-mode. Has an explicit review rubric covering correctness, modularity, error handling, test coverage, observability, and security smells.
- **`recon-security`** — security and compliance reviewer. Read-only. Carries the ISO 27001 / SOC 2 / FCA control list and reviews changes against it.
- **`recon-researcher`** — evaluates libraries, formats, and external standards. Read-only.

**`.claude/skills/*.skill`** — reusable skill packages for recurring capabilities:
- `reconciliation-matching-engine` — matching algorithm patterns, tolerance handling, fuzzy match scoring, replay determinism
- `multi-tenant-rust-backend` — tenant context propagation, request-scoped data, isolation tests
- `nextjs-design-system` — token usage, component primitives, accessibility patterns
- `compliance-control-mapping` — control IDs and how to evidence them in code
- `bank-statement-parsers` — MT940, BAI2, CAMT.053, CSV, PDF parsing patterns and test fixtures

---

## 5. Quality Bars

- **Justify every choice.** Every library, framework, and pattern includes the alternatives considered and the reason for the pick.
- **Modular and reusable.** Define the seams. Name the modules. Show how each is independently testable.
- **Security and testing are not separate phases.** They appear in every phase's exit criteria.
- **Observability lands in Phase 0** — structured logging, metrics, distributed tracing, and the audit-log pipeline skeleton, before any business logic.
- **The matching engine is deterministic and replayable.** Given the same inputs and configuration version, it produces the same outputs. Surface this in the design and the property-based tests.
- **Source records and match decisions are immutable.** Corrections are modelled as new events, never as edits.
- **No placeholder UI.** Design tokens, primitives, and the accessibility baseline are real on day one.
- **Multi-tenancy is cross-cutting.** It appears in architecture, backend, frontend, database, security, observability, and testing — not just one section.

---

## 6. Anti-patterns to Avoid

- A single monolithic plan document instead of the artefact bundle.
- Vague tasks like "implement authentication" — every task has acceptance criteria and verification commands.
- Compliance bolted onto a final phase.
- Generic Rust / Next.js advice not tied to this domain.
- Calendar-based estimates.
- Choosing tools without naming the rejected alternatives.
- Hiding multi-tenancy decisions inside the database section.
- "Migration to production" as the last task — production-readiness is gated per phase.
- Designing the matching engine as part of the HTTP layer. It is a separately testable library.
- Allowing the frontend to know backend implementation details.

---

## 7. Generation Order

Produce artefacts in this order so later files can reference earlier decisions without circularity:

1. `docs/architecture.md`
2. `docs/security-and-compliance.md`
3. `docs/database-and-infrastructure.md`
4. `docs/backend.md`
5. `docs/frontend.md`
6. `docs/observability-and-operations.md`
7. `docs/testing-strategy.md`
8. `docs/development-workflow.md`
9. `docs/implementation-plan.md`
10. `docs/todo/PHASE-0-foundations.md`, then subsequent phase TODO files
11. `.claude/agents/*.md`
12. `.claude/skills/*.skill`
13. `CLAUDE.md`
14. `README.md`
15. `docs/README.md` (the index, last, because it links everything)

---

## 8. Before You Start — Confirm Your Commitments

Before producing any artefact, output a short **Commitments Block** of no more than ten lines stating:

1. Scale tier targeted in Phase 0 (Launch by default, but confirm)
2. Database engine and version
3. Rust web framework and async runtime
4. Next.js rendering strategy (App Router + RSC / SSR / hybrid)
5. UI library / design system foundation
6. Multi-tenancy isolation model
7. API style (REST / GraphQL / RPC)
8. Background-job system
9. Cloud / infra posture (provider-specific or provider-agnostic, and which)
10. The single highest-risk assumption you are making

Once that block is produced, proceed to generate the artefacts in the order specified.

---

## 9. Definition of Done for This Prompt

You are done when:

- Every file listed in section 4 exists with content meeting the specification
- Every phase in the implementation plan has a corresponding TODO file with task IDs in the `REC-P{phase}-{nnn}` format
- Every TODO task has acceptance criteria, files touched, tests required, observability hooks, and verification commands
- Every architectural and library choice is justified with rejected alternatives
- The three compliance control-mapping tables are populated, not stubbed
- The `docs/README.md` index links every artefact with one-sentence descriptions
