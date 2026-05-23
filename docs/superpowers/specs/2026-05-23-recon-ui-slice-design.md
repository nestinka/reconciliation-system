# Design — Reconciliation Platform: Vertical UI Slice (Frontend-First)

- **Date:** 2026-05-23
- **Status:** Approved (pending written-spec review)
- **Scope:** Frontend-first vertical slice of the multi-tenant reconciliation platform described in `docs/reconciliation-platform-prompt.md`. UI built first against a typed mock data layer; real Rust backend wired in a later effort.

---

## 1. Goal & Non-Goals

### Goal
Build a polished, fully interactive, production-grade Next.js frontend covering the **core operator reconciliation loop**, demoable end-to-end against mock data, with a clean seam to swap in the real Rust HTTP backend later.

### Non-Goals (this slice)
- No real backend, database, or authentication (mocked).
- No ingestion config, connector management, matching-rule editor, reporting builder, or tenant-admin/RBAC screens (deferred — see §9).
- No real multi-tenancy enforcement; tenant context is a UI concern here, isolation is a backend concern landed later.
- No compliance controls implemented in code yet (the slice is structured so they *can* be — e.g. immutability modeled in UI — but ISO/SOC2/FCA controls land with the backend).

---

## 2. The Core Operator Loop (what we're building)

The slice covers the day-to-day loop of a reconciliation operator:

1. See overall reconciliation health → **Dashboard**
2. Inspect a reconciliation run and its match outcomes → **Runs list** + **Run detail**
3. Triage the breaks/exceptions that need attention → **Exceptions/Breaks list**
4. Investigate and resolve a break with a maker/checker control → **Investigation case detail (four-eyes)**

This is the "Core 5" screen set (app shell counts as the 5th surface).

---

## 3. Stack & Key Decisions

| Concern | Choice | Why (vs. alternatives) |
|---|---|---|
| Framework | **Next.js 16, App Router + RSC** | Brief mandates Next.js + "thin client over HTTP API". RSC by default, client components only where interactive. Rejected Pages Router (legacy), pure CSR SPA (loses streaming/SEO/server boundaries). |
| Language | **TypeScript (strict)** | Type-safe domain model is the contract for the future backend swap. |
| Styling | **Tailwind CSS v4** (CSS-first `@theme`) | User-requested. v4 token model fits a real design system. Rejected CSS Modules (slower system-building), styled-components (runtime cost, RSC friction). |
| Component foundation | **shadcn/ui (Radix + Tailwind), owned source** | Satisfies "design system from first commit" because the code is ours, not a runtime dep; Radix gives WCAG a11y baseline. Rejected hand-rolling on Radix (slow), MUI/Mantine/Ant (heavy runtime, fights dense bespoke look). |
| Server-state | **TanStack Query** | Mock client returns promises; swapping to real Rust API later = one-file change in the api client. Caching/loading/error/retry handled uniformly. |
| URL/filter state | **nuqs / searchParams** | Filters & table state shareable/bookmarkable; no global store needed for this slice. |
| Forms | **react-hook-form + zod** | Typed validation; zod schemas double as the contract shape for the backend later. |
| Charts | **Recharts** (dashboard only) | Lightweight, composable, good enough for KPI/break-analysis widgets. Rejected heavier viz libs (overkill for this slice). |
| Tests | **Vitest + Testing Library**, **Playwright** E2E, **axe** a11y | Component tests for primitives/screens; one E2E for the operator loop; automated a11y checks. |
| Package manager | **pnpm** | Already installed (9.15); fast, strict. |

Locked versions at design time: Next.js 16.2.6, React 19.2.6, Tailwind 4.3.0, Node 25.

---

## 4. Architecture

### The seam (most important property)
A single typed module `lib/api/client.ts` defines the `ApiClient` interface. The slice ships a `MockApiClient` implementation backed by in-memory fixtures with simulated latency. Every screen consumes the client **only** through TanStack Query hooks in `lib/hooks/`. The frontend never references backend internals.

Swapping to the real backend later = add `HttpApiClient implements ApiClient` and change one provider binding. Anti-pattern "frontend knows backend internals" is structurally prevented.

### Layering
- `lib/domain/` — canonical TypeScript types (the model). No React, no I/O.
- `lib/api/` — `ApiClient` interface + `MockApiClient` + fixtures.
- `lib/hooks/` — TanStack Query hooks (the only thing screens import for data).
- `components/ui/` — owned design-system primitives.
- `components/app/` — composed, domain-aware components.
- `app/` — routes/pages; thin, compose hooks + components.

### Directory structure
```
web/
  app/
    (app)/
      layout.tsx                 # app shell: nav, tenant switcher, dark mode
      dashboard/page.tsx
      runs/page.tsx              # runs list
      runs/[runId]/page.tsx      # run detail (tabs: matched/unmatched/partial/dupes)
      exceptions/page.tsx        # breaks list (cross-run)
      cases/[caseId]/page.tsx    # investigation + four-eyes
    layout.tsx                   # root: providers (Query, theme), fonts
    globals.css
  components/
    ui/                          # button, input, select, badge, table, dialog, tabs, toast, skeleton...
    app/                         # KpiCard, StatusPill, RunTable, BreakTable, CaseTimeline, ApprovalBar...
  lib/
    api/                         # client.ts (interface), mock.ts, fixtures.ts
    domain/                      # types.ts (canonical model), money.ts, date.ts, status.ts
    case/                        # approval.ts — four-eyes (maker/checker) domain logic
    hooks/                       # useRuns, useRun, useBreaks, useCase, useTenants...
    providers/                   # query-provider, theme-provider, tenant-provider, current-user
  styles/theme.css               # design tokens via @theme
  tests/                         # unit/component + e2e
```

---

## 5. Canonical Domain Model (TypeScript)

Mirrors the platform's intended canonical transaction model so the mock contract is forward-compatible.

- **Tenant** — `id, name, slug`.
- **Account / Source** — `id, kind: 'bank' | 'ledger' | 'cross_system', name, currency`.
- **CanonicalTransaction** — `id, tenantId, sourceId, externalRef, valueDate, postedAt, amountMinor, currency, direction: 'debit'|'credit', counterparty?, description, raw?`. **Immutable.**
- **ReconciliationRun** — `id, tenantId, name, sourceAId, sourceBId, status: 'running'|'completed'|'failed', startedAt, completedAt?, configVersion, stats: { matched, unmatched, partial, duplicate, breakCount, matchRatePct, valueAtRiskMinor }`.
- **MatchDecision** — `id, runId, type: 'matched'|'partial'|'duplicate', txnIds: string[], score, configVersion`. **Immutable.**
- **Exception / Break** — `id, tenantId, runId, type: 'unmatched'|'partial'|'duplicate'|'break', status: 'open'|'investigating'|'pending_approval'|'resolved'|'written_off', ageingDays, ageingBucket, valueMinor, currency, assigneeId?, txnIds, openedAt`.
- **Case** — investigation wrapper around a break: `id, breakId, assigneeId?, status, events: CaseEvent[]`.
- **CaseEvent** (append-only) — `comment | assignment | manual_match_proposed | write_off_proposed | approval_requested | approved | rejected`, each with `id, actorId, at, payload`. **Append-only; corrections are new events, never edits.**
- **User** — `id, name, role: 'operator'|'approver'|'admin'`.

Monetary amounts are integer minor units + currency (never floats).

---

## 6. Screens & Components

### App shell (`(app)/layout.tsx`)
Collapsible left nav (Dashboard, Runs, Exceptions), top bar with **tenant switcher**, dark-mode toggle, and current-user chip. Tenant context provided via `tenant-provider`; switching tenant refetches data.

### Dashboard
- KPI row: **match rate**, **open breaks**, **value-at-risk**, **SLA adherence**.
- **Break analysis by type** (bar/donut).
- **SLA / ageing** widget (breaks bucketed by age).
- **Recent runs** table (links into run detail).

### Runs list (`/runs`)
Dense sortable table: run name, source pair, status, match rate, break count, value-at-risk, completed time. Filters (status, source, date) held in URL via nuqs. Row → run detail.

### Run detail (`/runs/[runId]`)
Summary header (stats, config version) + **tabs**: Matched / Unmatched / Partial / Duplicates, each a dense table of transactions/decisions. Drilling into a break row opens/links its **case**.

### Exceptions/Breaks list (`/exceptions`)
All open breaks across runs: type, ageing bucket, assignee, value, status. Multi-select + **bulk assign**. Row → case detail.

### Investigation case detail (`/cases/[caseId]`)
- Break context panel (the unmatched/partial txns side by side).
- **Suggested matches** (mocked candidates with scores).
- Actions: **Assign**, **Comment**, **Propose manual match**, **Propose write-off**.
- **Four-eyes (maker/checker):** a maker proposes a resolution → case enters `pending_approval` → a *different* user (approver role) must Approve/Reject. UI enforces actor separation (maker ≠ checker) and shows a clear pending-approval state. All steps append to the **case timeline** (immutable event log).

---

## 7. Conventions

- **Status → color semantics:** matched=green, partial=amber, unmatched/break=red, pending=blue, written-off/neutral=gray. Color is **never the only signal** — always paired with label + icon (WCAG 2.2 AA).
- **Numerics:** tabular/monospace figures, right-aligned, currency-aware formatting from minor units.
- **Loading:** skeleton rows for tables; RSC suspense boundaries per segment.
- **Error:** typed error state + retry; never a blank screen.
- **Empty:** purposeful empty states with next action.
- **Immutability in UI:** write-offs and manual matches are rendered as *new timeline events*, never as edits to prior records.
- **Dark mode:** class-based, persisted; both themes are first-class (not an afterthought).
- **a11y baseline:** keyboard-navigable tables/dialogs, focus management, ARIA via Radix, `axe` checks in tests.

---

## 8. Testing Strategy (this slice)

- **Unit/component (Vitest + Testing Library):** every `ui/` primitive; key `app/` components (StatusPill semantics, RunTable sorting, ApprovalBar maker≠checker rule, CaseTimeline append-only render).
- **E2E (Playwright):** the operator loop — Dashboard → open a break → investigate → propose resolution (maker) → approve as a different user (checker) → break resolved.
- **a11y:** `axe` assertions on Dashboard, Runs, Run detail, Exceptions, Case detail.
- **Gates:** `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`, Playwright run — all green before "done".

### Verification commands
```
pnpm -C web lint
pnpm -C web typecheck
pnpm -C web test
pnpm -C web build
pnpm -C web exec playwright test
```

---

## 9. Deferred (and why)

| Deferred | Why |
|---|---|
| Real Rust backend, DB, migrations | Frontend-first by user direction; seam (§4) makes the swap localized. |
| Auth (real) | Mocked user/role is enough to demo four-eyes; real auth lands with backend. |
| Ingestion config, connectors, matching-rule editor | Not part of the core operator loop; separate slices. |
| Reporting builder, tenant admin, RBAC management | Out of Core 5; later phases. |
| Compliance controls in code (ISO/SOC2/FCA) | Land with the backend; UI is structured to support them (immutability, audit-friendly event log, four-eyes). |

---

## 10. Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Mock contract drifts from eventual backend | Domain types + zod schemas are the single contract source; keep them backend-agnostic and explicit. |
| "UI-first" mock data hides real-world matching complexity | Fixtures include partials, duplicates, ageing, and value-at-risk so the UI is stress-tested on realistic shapes. |
| shadcn/ui defaults look generic | Apply dense, data-first tokens + bespoke composed components; aesthetic owned, not default. |
| Scope creep into admin/config screens | §1 non-goals + §9 deferred list are the guardrails. |

---

## 11. Definition of Done (this slice)

- All Core 5 surfaces implemented and navigable against the mock layer.
- Four-eyes maker/checker flow enforced in UI (actor separation) and reflected in an append-only case timeline.
- Design tokens + accessible primitives in place; dark mode works; `axe` checks pass on key screens.
- All verification commands (§8) green.
- `ApiClient` seam in place so the backend can be wired with no screen changes.
