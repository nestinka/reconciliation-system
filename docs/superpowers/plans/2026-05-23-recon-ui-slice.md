# Reconciliation UI Slice — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a polished, interactive, production-grade Next.js frontend covering the core reconciliation operator loop (Dashboard → Runs → Run detail → Exceptions → Investigation with four-eyes), running against a typed mock data layer with a clean seam for the future Rust backend.

**Architecture:** Next.js 16 App Router + RSC. Every screen consumes data only through TanStack Query hooks, which call a single `ApiClient` interface. The slice ships a `MockApiClient` (in-memory fixtures + simulated latency); the real backend later implements the same interface with zero screen changes. Owned design-system primitives (shadcn/Radix + Tailwind v4 tokens) deliver a dense, data-first aesthetic with WCAG 2.2 AA + dark mode.

**Tech Stack:** Next.js 16.2.6, React 19.2.6, TypeScript (strict), Tailwind CSS v4.3, shadcn/ui (Radix), TanStack Query v5, nuqs, react-hook-form + zod, Recharts, Vitest + Testing Library, Playwright, pnpm.

**Reference spec:** `docs/superpowers/specs/2026-05-23-recon-ui-slice-design.md`

---

## File Structure (decomposition)

```
web/
  app/
    layout.tsx                      # root: providers, fonts, <html> theme
    globals.css                     # tailwind import + base
    (app)/
      layout.tsx                    # app shell (nav, tenant switcher, theme toggle)
      dashboard/page.tsx
      runs/page.tsx
      runs/[runId]/page.tsx
      exceptions/page.tsx
      cases/[caseId]/page.tsx
  styles/theme.css                  # @theme tokens
  lib/
    domain/types.ts                 # canonical model + zod schemas
    domain/money.ts                 # minor-unit currency formatting
    domain/status.ts                # status -> semantic mapping (single source)
    api/client.ts                   # ApiClient interface
    api/fixtures.ts                 # deterministic seed data
    api/mock.ts                     # MockApiClient implements ApiClient
    api/provider.tsx                # binds active ApiClient (mock now)
    hooks/use-tenants.ts
    hooks/use-runs.ts
    hooks/use-breaks.ts
    hooks/use-case.ts
    providers/query-provider.tsx
    providers/theme-provider.tsx
    providers/tenant-provider.tsx
    case/approval.ts                # four-eyes maker!=checker logic (pure, tested)
    utils/cn.ts
  components/
    ui/                             # button, badge, table, dialog, tabs, input, select, skeleton, card, toast, dropdown-menu, checkbox, avatar
    app/
      status-pill.tsx
      kpi-card.tsx
      page-header.tsx
      run-table.tsx
      break-table.tsx
      txn-table.tsx
      case-timeline.tsx
      approval-bar.tsx
      empty-state.tsx
      data-table.tsx                # shared dense table shell (sorting, selection)
  tests/
    e2e/operator-loop.spec.ts
  vitest.config.ts
  vitest.setup.ts
  playwright.config.ts
```

Each `lib/*` module is pure/React-free where possible (domain, money, status, approval) → trivially unit-testable. Screens stay thin.

---

## Conventions for every task

- **Commit** at the end of each task with a conventional-commit message.
- Run `pnpm -C web typecheck` and `pnpm -C web lint` before each commit; both must pass.
- Pure logic (domain/money/status/approval, table sorting, mock client) is **TDD**: failing test first.
- UI components get at least one component test asserting behavior/semantics, not snapshots.
- Commands assume repo root `/home/nestinka/assistant/reconciliation-system`.

---

## Task 0: Repo + Next.js scaffold

**Files:**
- Create: `.gitignore`, `web/` (via create-next-app), `web/package.json` (modified), config files below.

- [ ] **Step 1: Init git (no commit yet)**

```bash
cd /home/nestinka/assistant/reconciliation-system
git init
printf "node_modules/\n.next/\n.turbo/\ncoverage/\nplaywright-report/\ntest-results/\n*.log\n.DS_Store\n" > .gitignore
```

- [ ] **Step 2: Scaffold Next.js app non-interactively**

```bash
cd /home/nestinka/assistant/reconciliation-system
pnpm dlx create-next-app@latest web \
  --ts --app --tailwind --eslint --src-dir=false \
  --import-alias "@/*" --use-pnpm --turbopack --no-git
```
Expected: `web/` created with App Router, Tailwind v4, TS.

- [ ] **Step 3: Add runtime + dev dependencies**

```bash
cd /home/nestinka/assistant/reconciliation-system/web
pnpm add @tanstack/react-query nuqs react-hook-form zod @hookform/resolvers recharts class-variance-authority clsx tailwind-merge lucide-react next-themes
pnpm add -D vitest @vitejs/plugin-react jsdom @testing-library/react @testing-library/user-event @testing-library/jest-dom @playwright/test jest-axe @types/jest-axe vite-tsconfig-paths
```

- [ ] **Step 4: Initialize shadcn/ui**

```bash
cd /home/nestinka/assistant/reconciliation-system/web
pnpm dlx shadcn@latest init -d
pnpm dlx shadcn@latest add button badge table dialog tabs input select skeleton card dropdown-menu checkbox avatar sonner separator tooltip
```
Expected: components land in `components/ui/`, `lib/utils.ts` (cn) created.

- [ ] **Step 5: Add scripts to `web/package.json`**

Ensure `scripts` contains:
```json
{
  "dev": "next dev --turbopack",
  "build": "next build",
  "start": "next start",
  "lint": "next lint",
  "typecheck": "tsc --noEmit",
  "test": "vitest run",
  "test:watch": "vitest",
  "e2e": "playwright test"
}
```

- [ ] **Step 6: Vitest config**

Create `web/vitest.config.ts`:
```ts
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tsconfigPaths from "vite-tsconfig-paths";

export default defineConfig({
  plugins: [react(), tsconfigPaths()],
  test: {
    environment: "jsdom",
    setupFiles: ["./vitest.setup.ts"],
    globals: true,
    include: ["**/*.test.{ts,tsx}"],
    exclude: ["tests/e2e/**", "node_modules/**"],
  },
});
```
Create `web/vitest.setup.ts`:
```ts
import "@testing-library/jest-dom/vitest";
import { expect } from "vitest";
import * as axeMatchers from "jest-axe";
expect.extend(axeMatchers.default ?? {});
```

- [ ] **Step 7: Playwright config**

Create `web/playwright.config.ts`:
```ts
import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./tests/e2e",
  use: { baseURL: "http://localhost:3000" },
  webServer: {
    command: "pnpm dev",
    url: "http://localhost:3000",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
```

- [ ] **Step 8: Verify scaffold builds**

```bash
cd /home/nestinka/assistant/reconciliation-system/web
pnpm typecheck && pnpm build
```
Expected: typecheck clean, build succeeds.

- [ ] **Step 9: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add -A && git commit -m "chore: scaffold Next.js 16 + Tailwind v4 + tooling"
```

---

## Task 1: Design tokens & theme

**Files:**
- Create: `web/styles/theme.css`
- Modify: `web/app/globals.css`

- [ ] **Step 1: Define tokens** in `web/styles/theme.css` using Tailwind v4 `@theme` — dense data-first palette with semantic status colors (success/warning/danger/info/neutral), tabular numerics font feature, tight spacing scale, radius scale, and dark-mode variant via `.dark` class. Include `--font-mono` for numerics.

```css
@theme {
  --color-bg: oklch(99% 0 0);
  --color-surface: oklch(97% 0.003 250);
  --color-border: oklch(90% 0.004 250);
  --color-fg: oklch(20% 0.01 250);
  --color-muted: oklch(50% 0.01 250);
  --color-success: oklch(62% 0.15 150);
  --color-warning: oklch(72% 0.15 75);
  --color-danger: oklch(60% 0.20 25);
  --color-info: oklch(60% 0.15 250);
  --radius-sm: 0.25rem;
  --radius-md: 0.375rem;
}
.dark {
  --color-bg: oklch(18% 0.01 250);
  --color-surface: oklch(22% 0.01 250);
  --color-border: oklch(30% 0.01 250);
  --color-fg: oklch(96% 0.005 250);
  --color-muted: oklch(65% 0.01 250);
}
```

- [ ] **Step 2:** `@import "../styles/theme.css";` into `globals.css`; set base `body` bg/fg, enable `font-variant-numeric: tabular-nums` utility class `.nums`.

- [ ] **Step 3: Verify** `pnpm -C web build` succeeds.

- [ ] **Step 4: Commit** `style: design tokens + dark mode theme`.

---

## Task 2: Status semantics (TDD)

**Files:**
- Create: `web/lib/domain/status.ts`, `web/lib/domain/status.test.ts`

- [ ] **Step 1: Failing test** `web/lib/domain/status.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { statusMeta } from "./status";

describe("statusMeta", () => {
  it("maps matched to success with a label and icon", () => {
    const m = statusMeta("matched");
    expect(m.tone).toBe("success");
    expect(m.label).toBe("Matched");
    expect(m.icon).toBeTruthy();
  });
  it("maps break to danger", () => {
    expect(statusMeta("break").tone).toBe("danger");
  });
  it("never relies on color alone (always has a label)", () => {
    for (const s of ["matched","partial","unmatched","break","pending_approval","resolved","written_off"] as const) {
      expect(statusMeta(s).label.length).toBeGreaterThan(0);
    }
  });
});
```

- [ ] **Step 2: Run, expect FAIL** `pnpm -C web test status` → module not found.

- [ ] **Step 3: Implement** `status.ts`: a `StatusKind` union and `statusMeta(kind)` returning `{ tone: 'success'|'warning'|'danger'|'info'|'neutral', label: string, icon: LucideIcon }`. Single source of truth for status→color/label/icon.

- [ ] **Step 4: Run, expect PASS.**

- [ ] **Step 5: Commit** `feat: status semantics single source of truth`.

---

## Task 3: Money formatting (TDD)

**Files:**
- Create: `web/lib/domain/money.ts`, `web/lib/domain/money.test.ts`

- [ ] **Step 1: Failing test**:
```ts
import { describe, it, expect } from "vitest";
import { formatMoney } from "./money";

describe("formatMoney", () => {
  it("formats minor units with currency", () => {
    expect(formatMoney(123456, "GBP")).toBe("£1,234.56");
  });
  it("handles zero-decimal currencies", () => {
    expect(formatMoney(1000, "JPY")).toBe("¥1,000");
  });
  it("formats negatives", () => {
    expect(formatMoney(-5000, "USD")).toBe("-$50.00");
  });
});
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Implement** `formatMoney(amountMinor: number, currency: string)` using `Intl.NumberFormat`, deriving fraction digits from the currency.

- [ ] **Step 4: Run, expect PASS.**

- [ ] **Step 5: Commit** `feat: minor-unit money formatting`.

---

## Task 4: Canonical domain types

**Files:**
- Create: `web/lib/domain/types.ts`

- [ ] **Step 1:** Define TS types + zod schemas exactly as in spec §5: `Tenant`, `Source` (`kind`), `CanonicalTransaction`, `ReconciliationRun` (+`RunStats`), `MatchDecision`, `Break` (Exception), `Case`, `CaseEvent` (discriminated union by `kind`), `User` (`role`). Money as integer `*Minor` fields. Export `zod` schemas alongside types (schemas are the backend contract source).

- [ ] **Step 2: Verify** `pnpm -C web typecheck` clean.

- [ ] **Step 3: Commit** `feat: canonical domain model + zod schemas`.

---

## Task 5: Four-eyes approval logic (TDD)

**Files:**
- Create: `web/lib/case/approval.ts`, `web/lib/case/approval.test.ts`

- [ ] **Step 1: Failing test** — encodes the maker≠checker rule and role requirement:
```ts
import { describe, it, expect } from "vitest";
import { canApprove, requestApproval } from "./approval";
import type { Case, User } from "@/lib/domain/types";

const maker: User = { id: "u1", name: "Mia", role: "operator" };
const checker: User = { id: "u2", name: "Theo", role: "approver" };

const pendingCase = (): Case => ({
  id: "c1", breakId: "b1", assigneeId: "u1", status: "pending_approval",
  events: [{ id: "e1", kind: "approval_requested", actorId: "u1", at: "2026-05-23T10:00:00Z", payload: { resolution: "write_off" } }],
});

describe("four-eyes", () => {
  it("rejects the maker approving their own proposal", () => {
    expect(canApprove(pendingCase(), maker).allowed).toBe(false);
  });
  it("allows a different approver to approve", () => {
    expect(canApprove(pendingCase(), checker).allowed).toBe(true);
  });
  it("blocks approval when not pending", () => {
    const c = { ...pendingCase(), status: "open" as const };
    expect(canApprove(c, checker).allowed).toBe(false);
  });
  it("requestApproval appends an approval_requested event and sets pending", () => {
    const open: Case = { id: "c1", breakId: "b1", assigneeId: "u1", status: "investigating", events: [] };
    const next = requestApproval(open, maker, "write_off");
    expect(next.status).toBe("pending_approval");
    expect(next.events.at(-1)?.kind).toBe("approval_requested");
    expect(next.events.length).toBe(open.events.length + 1); // append-only
  });
});
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Implement** `approval.ts`:
  - `canApprove(c: Case, user: User): { allowed: boolean; reason?: string }` — allowed only if `c.status === 'pending_approval'`, `user.role` is `approver`/`admin`, and the original `approval_requested` actor ≠ `user.id`.
  - `requestApproval(c, maker, resolution): Case` — pure; returns a new case with appended `approval_requested` event and `status: 'pending_approval'` (never mutates).
  - `approve` / `reject` similarly append events and set `resolved`/back to `investigating`.

- [ ] **Step 4: Run, expect PASS.**

- [ ] **Step 5: Commit** `feat: four-eyes maker/checker approval logic`.

---

## Task 6: ApiClient interface + fixtures + MockApiClient (TDD)

**Files:**
- Create: `web/lib/api/client.ts`, `web/lib/api/fixtures.ts`, `web/lib/api/mock.ts`, `web/lib/api/mock.test.ts`, `web/lib/api/provider.tsx`

- [ ] **Step 1: Define `ApiClient` interface** in `client.ts` — the seam. Methods (all `Promise`, all tenant-scoped):
```ts
export interface ApiClient {
  listTenants(): Promise<Tenant[]>;
  getDashboard(tenantId: string): Promise<DashboardSummary>;
  listRuns(tenantId: string, q?: RunQuery): Promise<ReconciliationRun[]>;
  getRun(tenantId: string, runId: string): Promise<RunDetail>;
  listBreaks(tenantId: string, q?: BreakQuery): Promise<Break[]>;
  getCase(tenantId: string, caseId: string): Promise<{ case: Case; brk: Break; suggestions: MatchSuggestion[] }>;
  assignBreak(tenantId: string, breakId: string, userId: string): Promise<Break>;
  appendCaseEvent(tenantId: string, caseId: string, event: NewCaseEvent): Promise<Case>;
}
```
Define `DashboardSummary`, `RunDetail`, `RunQuery`, `BreakQuery`, `MatchSuggestion`, `NewCaseEvent` here.

- [ ] **Step 2: Fixtures** `fixtures.ts` — deterministic seed: 2 tenants, ~6 sources, ~8 runs across statuses with realistic `RunStats` (incl. partials/dupes/value-at-risk), ~40 transactions, ~15 breaks across ageing buckets/types, 1 case in `pending_approval` for the E2E. Pure data, no randomness (replay determinism).

- [ ] **Step 3: Failing test** `mock.test.ts`:
```ts
import { describe, it, expect } from "vitest";
import { MockApiClient } from "./mock";

const api = new MockApiClient({ latencyMs: 0 });

describe("MockApiClient", () => {
  it("scopes runs by tenant", async () => {
    const [t1] = await api.listTenants();
    const runs = await api.listRuns(t1.id);
    expect(runs.every(r => r.tenantId === t1.id)).toBe(true);
  });
  it("filters breaks by status", async () => {
    const [t1] = await api.listTenants();
    const open = await api.listBreaks(t1.id, { status: "open" });
    expect(open.every(b => b.status === "open")).toBe(true);
  });
  it("appendCaseEvent is append-only and immutable", async () => {
    const [t1] = await api.listTenants();
    const { case: c } = await api.getCase(t1.id, "case-pending");
    const before = c.events.length;
    const next = await api.appendCaseEvent(t1.id, c.id, { kind: "comment", actorId: "u1", payload: { text: "looking" } });
    expect(next.events.length).toBe(before + 1);
  });
});
```

- [ ] **Step 4: Run, expect FAIL.**

- [ ] **Step 5: Implement `MockApiClient`** in `mock.ts` from fixtures: tenant scoping, query filtering (status/type/source/date for runs & breaks), simulated latency via `await sleep(latencyMs)`, and append-only `appendCaseEvent` (returns new arrays, never mutates fixtures). Reuses `approval.ts` for approval events.

- [ ] **Step 6: Run, expect PASS.**

- [ ] **Step 7: Provider** `provider.tsx` — `ApiProvider` exposing the active client via context, defaulting to `new MockApiClient()`; `useApi()` hook.

- [ ] **Step 8: Commit** `feat: ApiClient seam + deterministic mock backend`.

---

## Task 7: Providers + query hooks

**Files:**
- Create: `web/lib/providers/query-provider.tsx`, `theme-provider.tsx`, `tenant-provider.tsx`, `web/lib/hooks/*.ts`
- Modify: `web/app/layout.tsx`

- [ ] **Step 1:** `query-provider.tsx` — client component wrapping `QueryClientProvider` with a singleton `QueryClient`. `theme-provider.tsx` — wrap `next-themes` (`attribute="class"`, default system). `tenant-provider.tsx` — holds `activeTenantId`, persisted to `localStorage`, exposes `useTenant()` + setter.

- [ ] **Step 2:** Hooks: `use-tenants` (`listTenants`), `use-runs` (`listRuns`/`getRun`), `use-breaks` (`listBreaks` with query), `use-case` (`getCase`, plus mutations `assignBreak`/`appendCaseEvent` that invalidate). Each reads `useApi()` + `useTenant()`. Query keys include tenantId.

- [ ] **Step 3:** Compose providers in root `app/layout.tsx` (ThemeProvider > QueryProvider > ApiProvider > TenantProvider), add `<NuqsAdapter>`, set `lang`, `suppressHydrationWarning` on `<html>`, load Inter + a mono font.

- [ ] **Step 4: Verify** `pnpm -C web typecheck && pnpm -C web build`.

- [ ] **Step 5: Commit** `feat: providers + TanStack Query hooks`.

---

## Task 8: Shared UI app components

**Files:**
- Create: `web/components/app/status-pill.tsx` (+ `.test.tsx`), `kpi-card.tsx`, `page-header.tsx`, `empty-state.tsx`, `data-table.tsx`

- [ ] **Step 1: Failing test** `status-pill.test.tsx`:
```tsx
import { render, screen } from "@testing-library/react";
import { StatusPill } from "./status-pill";

it("renders label text (not color-only) for accessibility", () => {
  render(<StatusPill status="break" />);
  expect(screen.getByText("Break")).toBeInTheDocument();
});
```

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3: Implement** `StatusPill` using `statusMeta` (tone→token class, renders icon + label). Then `KpiCard` (label, value, delta, optional sparkline), `PageHeader` (title, description, actions slot), `EmptyState`, and `DataTable` (generic dense table: columns config, optional sort + row selection, sticky header, skeleton + empty states).

- [ ] **Step 4: Run, expect PASS.**

- [ ] **Step 5: Commit** `feat: shared app components (status pill, kpi, data table)`.

---

## Task 9: App shell

**Files:**
- Create: `web/app/(app)/layout.tsx`, supporting nav + tenant switcher + theme toggle components in `components/app/`.

- [ ] **Step 1:** Build shell: collapsible left nav (Dashboard `/dashboard`, Runs `/runs`, Exceptions `/exceptions`) with active-state; top bar with **tenant switcher** (dropdown from `use-tenants`, sets `useTenant`), **theme toggle** (next-themes), user chip. Responsive (nav collapses on narrow).

- [ ] **Step 2: Component test** asserting nav links + tenant switcher render and switching updates context.

- [ ] **Step 3: Verify** `pnpm -C web build`.

- [ ] **Step 4: Commit** `feat: application shell with tenant switcher + theme toggle`.

---

## Task 10: Dashboard

**Files:**
- Create: `web/app/(app)/dashboard/page.tsx`, `web/components/app/break-analysis-chart.tsx`, `ageing-widget.tsx`

- [ ] **Step 1:** Page uses `getDashboard` via hook: KPI row (match rate, open breaks, value-at-risk, SLA adherence) with `KpiCard`; break-analysis-by-type chart (Recharts); ageing widget; recent-runs table (`RunTable` minimal) linking to `/runs/[id]`. Loading skeletons + empty states.

- [ ] **Step 2: Component test** rendering dashboard with a test QueryClient + mock api asserting KPIs appear.

- [ ] **Step 3: Verify + Commit** `feat: dashboard screen`.

---

## Task 11: Runs list + run detail

**Files:**
- Create: `web/components/app/run-table.tsx`, `txn-table.tsx`; `web/app/(app)/runs/page.tsx`, `web/app/(app)/runs/[runId]/page.tsx`

- [ ] **Step 1:** `RunTable` on `DataTable`: columns name, source pair, status (`StatusPill`), match rate, breaks, value-at-risk, completed. URL filters (status/source/date) via nuqs.

- [ ] **Step 2:** Runs list page wires `use-runs` + filters + sorting.

- [ ] **Step 3:** Run detail: header (stats, configVersion) + Tabs (Matched/Unmatched/Partial/Duplicates), each a `TxnTable`; unmatched/partial rows link to their case `/cases/[caseId]`.

- [ ] **Step 4: Component test** asserting tab switching shows the right rows + sort works.

- [ ] **Step 5: Verify + Commit** `feat: runs list and run detail`.

---

## Task 12: Exceptions/Breaks list

**Files:**
- Create: `web/components/app/break-table.tsx`; `web/app/(app)/exceptions/page.tsx`

- [ ] **Step 1:** `BreakTable` on `DataTable` with selection: type, ageing bucket, assignee, value, status. Bulk **assign** action (calls `assignBreak` mutation). URL filters (type/status/ageing/assignee) via nuqs. Row → `/cases/[caseId]`.

- [ ] **Step 2: Component test** asserting filter narrows rows and bulk-assign calls mutation.

- [ ] **Step 3: Verify + Commit** `feat: exceptions/breaks list with bulk assign`.

---

## Task 13: Investigation case detail + four-eyes UI

**Files:**
- Create: `web/components/app/case-timeline.tsx`, `approval-bar.tsx`; `web/app/(app)/cases/[caseId]/page.tsx`

- [ ] **Step 1: Component test** `approval-bar.test.tsx` — given a `pending_approval` case and the maker as current user, the Approve button is disabled with an explanatory message; given a different approver, it's enabled (drives from `canApprove`).

- [ ] **Step 2: Run, expect FAIL.**

- [ ] **Step 3:** Build case page: break context (side-by-side txns), suggested matches (scored), action bar (Assign, Comment, Propose manual match, Propose write-off → `requestApproval`), `CaseTimeline` (append-only event render), and `ApprovalBar` (uses `canApprove`; Approve/Reject append events; maker≠checker enforced; clear pending state). A "current user" switcher (dev affordance) lets the demo swap maker/checker to exercise four-eyes.

- [ ] **Step 4: Run tests, expect PASS.**

- [ ] **Step 5: Verify + Commit** `feat: investigation case detail with four-eyes approval`.

---

## Task 14: E2E + a11y + final verification

**Files:**
- Create: `web/tests/e2e/operator-loop.spec.ts`

- [ ] **Step 1:** Playwright E2E for the operator loop: Dashboard → Exceptions → open a `pending_approval` break's case → as maker, Approve is blocked → switch current user to approver → Approve → case shows resolved + timeline gains `approved` event.

- [ ] **Step 2:** Add `axe` assertions in component tests for Dashboard, Runs, Run detail, Exceptions, Case detail (no serious/critical violations).

- [ ] **Step 3: Run full gate**

```bash
cd /home/nestinka/assistant/reconciliation-system/web
pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm exec playwright install --with-deps chromium && pnpm e2e
```
Expected: all green.

- [ ] **Step 4: Commit** `test: e2e operator loop + a11y checks`.

---

## Self-Review

**Spec coverage:** Dashboard (Task 10), Runs list+detail (11), Exceptions (12), Case detail + four-eyes (13), app shell + tenant switcher + dark mode (9), seam/ApiClient (6), domain model (4), money/status conventions (2,3), immutability/append-only (5,6,13), testing incl. a11y + E2E (14). All spec §6/§7/§8 items mapped. Deferred items (§9) intentionally absent.

**Placeholder scan:** No TBD/TODO; pure-logic tasks carry full test code; screen tasks specify exact columns/actions/wiring. Representative code given for all contract-defining modules.

**Type consistency:** `ApiClient` method names, `statusMeta`/`StatusKind`, `canApprove`/`requestApproval`, `Break`/`Case`/`CaseEvent`, `*Minor` money fields are consistent across tasks 2–13.
