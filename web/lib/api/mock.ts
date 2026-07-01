import type { Break, Case, CaseEvent, CanonicalTransaction, ReconciliationRun, Source, User } from "@/lib/domain/types";
import { approve, reject, requestApproval } from "@/lib/case/approval";
import type {
  Anchor,
  ApiClient,
  AuditEvent,
  AuditPage,
  AuditQuery,
  BreakQuery,
  Control,
  CreateUserInput,
  CreateRunInput,
  CreateSourceInput,
  CsvMapping,
  DashboardSummary,
  IngestFormat,
  IngestResult,
  MatchSuggestion,
  NewCaseEvent,
  RunDetail,
  RunQuery,
  SourceListItem,
  UpdateSourceInput,
  UpdateUserPatch,
  VerifyRequest,
  VerifyResult,
} from "./client";
import { buildFixtures, type Fixtures } from "./fixtures";

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function deepClone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value));
}

function nextId(): string {
  return crypto.randomUUID();
}

export class MockApiClient implements ApiClient {
  private readonly latencyMs: number;
  private state: Fixtures;
  private auditEvents: AuditEvent[];

  constructor(opts?: { latencyMs?: number }) {
    this.latencyMs = opts?.latencyMs ?? 150;
    this.state = deepClone(buildFixtures());
    // Three deterministic sample events for tenant-acme. Hashes are stub hex.
    this.auditEvents = [
      {
        tenantId: "tenant-acme",
        seq: 1,
        at: "2026-05-20T08:00:00Z",
        actorId: "user-ada",
        kind: "auth.login.success",
        payload: { ip: "203.0.113.7" },
        prevHash: "0".repeat(64),
        hash: "a".repeat(64),
      },
      {
        tenantId: "tenant-acme",
        seq: 2,
        at: "2026-05-20T09:15:00Z",
        actorId: "user-sam",
        kind: "data.ingest.completed",
        payload: { sourceId: "src-acme-bank", rows: 120 },
        prevHash: "a".repeat(64),
        hash: "b".repeat(64),
      },
      {
        tenantId: "tenant-acme",
        seq: 3,
        at: "2026-05-20T10:30:00Z",
        actorId: "user-theo",
        kind: "case.assigned",
        payload: { caseId: "case-001", assigneeId: "user-mia" },
        prevHash: "b".repeat(64),
        hash: "c".repeat(64),
      },
    ];
  }

  private async delay(): Promise<void> {
    if (this.latencyMs > 0) {
      await sleep(this.latencyMs);
    }
  }

  // -------------------------------------------------------------------------
  // Tenants
  // -------------------------------------------------------------------------

  async listTenants() {
    await this.delay();
    return deepClone(this.state.tenants);
  }

  // -------------------------------------------------------------------------
  // Users
  // -------------------------------------------------------------------------

  async listUsers(
    tenantId: string // eslint-disable-line @typescript-eslint/no-unused-vars
  ) {
    await this.delay();
    // All users are available for all tenants in this dev fixture
    return deepClone(this.state.users);
  }

  async listMembers(
    tenantId: string // eslint-disable-line @typescript-eslint/no-unused-vars
  ) {
    await this.delay();
    // Same fixture users — non-privileged read
    return deepClone(this.state.users);
  }

  async createUser(
    _tenantId: string,
    input: CreateUserInput
  ): Promise<User> {
    await this.delay();
    const newUser: User = {
      id: nextId(),
      name: input.name,
      email: input.email,
      role: input.role,
      disabled: false,
    };
    this.state.users.push(newUser);
    return deepClone(newUser);
  }

  async updateUser(
    _tenantId: string,
    userId: string,
    patch: UpdateUserPatch
  ): Promise<void> {
    await this.delay();
    const idx = this.state.users.findIndex((u) => u.id === userId);
    if (idx === -1) throw new Error(`User "${userId}" not found.`);
    this.state.users[idx] = { ...this.state.users[idx], ...patch };
  }

  async deleteUser(
    _tenantId: string,
    userId: string
  ): Promise<void> {
    await this.delay();
    const idx = this.state.users.findIndex((u) => u.id === userId);
    if (idx === -1) throw new Error(`User "${userId}" not found.`);
    this.state.users.splice(idx, 1);
  }

  // -------------------------------------------------------------------------
  // Dashboard
  // -------------------------------------------------------------------------

  async getDashboard(tenantId: string): Promise<DashboardSummary> {
    await this.delay();

    const breaks = this.state.breaks.filter((b) => b.tenantId === tenantId);
    const runs = this.state.runs.filter((r) => r.tenantId === tenantId);

    const openBreaks = breaks.filter(
      (b) => b.status === "open" || b.status === "investigating" || b.status === "pending_approval"
    );

    const valueAtRiskMinor = openBreaks.reduce((sum, b) => sum + b.valueMinor, 0);

    // Determine primary currency from first run's source
    const currency = breaks[0]?.currency ?? "GBP";

    const completedRuns = runs.filter((r) => r.status === "completed");
    const avgMatchRate =
      completedRuns.length > 0
        ? completedRuns.reduce((sum, r) => sum + r.stats.matchRatePct, 0) /
          completedRuns.length
        : 0;

    // SLA: breaks resolved within 7 days out of total resolved
    const resolvedBreaks = breaks.filter(
      (b) => b.status === "resolved" || b.status === "written_off"
    );
    const slaAdherent = resolvedBreaks.filter((b) => b.ageingDays <= 7);
    const slaAdherencePct =
      resolvedBreaks.length > 0
        ? (slaAdherent.length / resolvedBreaks.length) * 100
        : 100;

    const breaksByType = (
      ["unmatched", "partial", "duplicate", "break"] as const
    ).map((type) => ({
      type,
      count: breaks.filter((b) => b.type === type).length,
    }));

    const breaksByAgeing = (
      ["0-1d", "2-7d", "8-30d", "30d+"] as const
    ).map((bucket) => ({
      bucket,
      count: openBreaks.filter((b) => b.ageingBucket === bucket).length,
    }));

    const recentRuns = deepClone(
      runs
        .filter((r) => r.status === "completed")
        .sort((a, b) => b.startedAt.localeCompare(a.startedAt))
        .slice(0, 5)
    );

    return {
      matchRatePct: Math.round(avgMatchRate * 10) / 10,
      openBreaks: openBreaks.length,
      valueAtRiskMinor,
      currency,
      slaAdherencePct: Math.round(slaAdherencePct * 10) / 10,
      breaksByType,
      breaksByAgeing,
      recentRuns,
    };
  }

  // -------------------------------------------------------------------------
  // Runs
  // -------------------------------------------------------------------------

  async listRuns(tenantId: string, q?: RunQuery) {
    await this.delay();

    let runs = this.state.runs.filter((r) => r.tenantId === tenantId);

    if (q?.status) {
      runs = runs.filter((r) => r.status === q.status);
    }
    if (q?.sourceId) {
      runs = runs.filter(
        (r) => r.sourceAId === q.sourceId || r.sourceBId === q.sourceId
      );
    }
    if (q?.from) {
      runs = runs.filter((r) => r.startedAt >= q.from!);
    }
    if (q?.to) {
      runs = runs.filter((r) => r.startedAt <= q.to!);
    }

    return deepClone(runs);
  }

  async getRun(tenantId: string, runId: string): Promise<RunDetail> {
    await this.delay();

    const run = this.state.runs.find(
      (r) => r.id === runId && r.tenantId === tenantId
    );
    if (!run) {
      throw new Error(`Run "${runId}" not found for tenant "${tenantId}".`);
    }

    const decisions = this.state.matchDecisions.filter(
      (md) => md.runId === runId
    );
    const matched = decisions.filter((md) => md.type === "matched");
    const partial = decisions.filter((md) => md.type === "partial");
    const duplicates = decisions.filter((md) => md.type === "duplicate");

    const unmatched = this.state.breaks.filter(
      (b) => b.runId === runId && b.tenantId === tenantId
    );

    // Collect all referenced transaction ids
    const allTxnIds = new Set<string>([
      ...decisions.flatMap((md) => md.txnIds),
      ...unmatched.flatMap((b) => b.txnIds),
    ]);

    const transactionsById: Record<string, CanonicalTransaction> = {};
    for (const txn of this.state.transactions) {
      if (allTxnIds.has(txn.id)) {
        transactionsById[txn.id] = txn;
      }
    }

    return deepClone({
      run,
      transactionsById,
      matched,
      partial,
      duplicates,
      unmatched,
    });
  }

  // -------------------------------------------------------------------------
  // Breaks
  // -------------------------------------------------------------------------

  async listBreaks(tenantId: string, q?: BreakQuery) {
    await this.delay();

    let breaks = this.state.breaks.filter((b) => b.tenantId === tenantId);

    if (q?.status) {
      breaks = breaks.filter((b) => b.status === q.status);
    }
    if (q?.type) {
      breaks = breaks.filter((b) => b.type === q.type);
    }
    if (q?.ageingBucket) {
      breaks = breaks.filter((b) => b.ageingBucket === q.ageingBucket);
    }
    if (q?.assigneeId) {
      breaks = breaks.filter((b) => b.assigneeId === q.assigneeId);
    }

    return deepClone(breaks);
  }

  // -------------------------------------------------------------------------
  // Cases
  // -------------------------------------------------------------------------

  async getCase(
    tenantId: string,
    caseId: string
  ): Promise<{
    case: Case;
    brk: Break;
    suggestions: MatchSuggestion[];
    transactionsById: Record<string, CanonicalTransaction>;
  }> {
    await this.delay();

    const brk = this.state.breaks.find(
      (b) => b.caseId === caseId && b.tenantId === tenantId
    );
    if (!brk) {
      throw new Error(
        `Case "${caseId}" not found for tenant "${tenantId}".`
      );
    }

    const c = this.state.cases.find((cs) => cs.id === caseId);
    if (!c) {
      throw new Error(`Case "${caseId}" data not found.`);
    }

    // Return suggestions relevant to this case's break transactions
    const suggestions = this.state.suggestions.filter((s) =>
      brk.txnIds.some((tid) => s.txnIds.includes(tid))
    );

    // Collect all referenced transaction ids: break's txnIds + every suggestion's txnIds
    const allTxnIds = new Set<string>([
      ...brk.txnIds,
      ...suggestions.flatMap((s) => s.txnIds),
    ]);
    const transactionsById: Record<string, CanonicalTransaction> = {};
    for (const txn of this.state.transactions) {
      if (allTxnIds.has(txn.id)) {
        transactionsById[txn.id] = txn;
      }
    }

    return deepClone({ case: c, brk, suggestions, transactionsById });
  }

  // -------------------------------------------------------------------------
  // assignBreak
  // -------------------------------------------------------------------------

  async assignBreak(
    tenantId: string,
    breakId: string,
    userId: string
  ): Promise<Break> {
    await this.delay();

    const brkIdx = this.state.breaks.findIndex(
      (b) => b.id === breakId && b.tenantId === tenantId
    );
    if (brkIdx === -1) {
      throw new Error(
        `Break "${breakId}" not found for tenant "${tenantId}".`
      );
    }

    const brk = this.state.breaks[brkIdx];
    const caseIdx = this.state.cases.findIndex((c) => c.id === brk.caseId);

    // Update break. Only an "open" break advances to "investigating" on
    // assignment; already-progressed breaks (investigating/pending_approval/
    // resolved/written_off) keep their status and just gain an assignee.
    const newStatus =
      brk.status === "open" ? "investigating" : brk.status;
    this.state.breaks[brkIdx] = {
      ...brk,
      assigneeId: userId,
      status: newStatus,
    };

    // Update case
    if (caseIdx !== -1) {
      const c = this.state.cases[caseIdx];
      const assignmentEvent = {
        id: nextId(),
        kind: "assignment" as const,
        actorId: userId,
        at: new Date().toISOString(),
        payload: { assigneeId: userId },
      };
      this.state.cases[caseIdx] = {
        ...c,
        assigneeId: userId,
        status: c.status === "open" ? "investigating" : c.status,
        events: [...c.events, assignmentEvent],
      };
    }

    return deepClone(this.state.breaks[brkIdx]);
  }

  // -------------------------------------------------------------------------
  // Sources
  // -------------------------------------------------------------------------

  async listPdfProfiles(_tenantId: string): Promise<string[]> {
    await this.delay();
    return ["acmebank"];
  }

  async listSources(tenantId: string, includeArchived?: boolean): Promise<SourceListItem[]> {
    await this.delay();
    return this.state.sources
      .filter((s) => s.tenantId === tenantId && (includeArchived || !s.disabled))
      .map((s) =>
        deepClone({
          ...s,
          txnCount: this.state.transactions.filter((t) => t.sourceId === s.id).length,
        })
      );
  }

  async archiveSource(tenantId: string, sourceId: string): Promise<void> {
    await this.delay();
    const s = this.state.sources.find((x) => x.id === sourceId && x.tenantId === tenantId);
    if (s) s.disabled = true;
  }

  async restoreSource(tenantId: string, sourceId: string): Promise<void> {
    await this.delay();
    const s = this.state.sources.find((x) => x.id === sourceId && x.tenantId === tenantId);
    if (s) s.disabled = false;
  }

  async createSource(tenantId: string, input: CreateSourceInput): Promise<Source> {
    await this.delay();
    const src: Source = {
      id: `src-${nextId()}`,
      tenantId,
      kind: input.kind,
      name: input.name,
      currency: input.currency,
      formatDialect: input.formatDialect ?? null,
      pdfProfile: input.pdfProfile ?? null,
      disabled: false,
    };
    this.state.sources.push(deepClone(src));
    return src;
  }

  async updateSource(
    tenantId: string,
    sourceId: string,
    patch: UpdateSourceInput,
  ): Promise<Source> {
    await this.delay();
    const idx = this.state.sources.findIndex(
      (s) => s.id === sourceId && s.tenantId === tenantId,
    );
    if (idx === -1) {
      throw new Error(`Source "${sourceId}" not found for tenant "${tenantId}".`);
    }
    const existing = this.state.sources[idx];
    const updated: Source = {
      ...existing,
      name: patch.name !== undefined ? patch.name : existing.name,
      formatDialect: patch.formatDialect === undefined
        ? existing.formatDialect
        : patch.formatDialect,
      pdfProfile: patch.pdfProfile === undefined ? existing.pdfProfile : patch.pdfProfile,
    };
    this.state.sources[idx] = updated;
    return deepClone(updated);
  }

  async ingestFile(
    tenantId: string,
    sourceId: string,
    format: IngestFormat, // eslint-disable-line @typescript-eslint/no-unused-vars
    file: File, // eslint-disable-line @typescript-eslint/no-unused-vars
    mapping?: CsvMapping, // eslint-disable-line @typescript-eslint/no-unused-vars
    dialect?: string, // eslint-disable-line @typescript-eslint/no-unused-vars
    pdfProfile?: string // eslint-disable-line @typescript-eslint/no-unused-vars
  ): Promise<IngestResult> {
    await this.delay();
    // The mock does not parse real bytes; it records a deterministic ingest so
    // UI flows (success summary) can be tested. One synthetic txn per call.
    const ref = `MOCK-${nextId()}`;
    this.state.transactions.push(deepClone({
      id: `txn-${nextId()}`, tenantId, sourceId,
      externalRef: ref, valueDate: "2026-05-10", postedAt: "2026-05-10T00:00:00Z",
      amountMinor: 1000, currency: "GBP", direction: "debit", description: "Mock ingest",
    }) as CanonicalTransaction);
    return { ingested: 1, sourceId };
  }

  async createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun> {
    await this.delay();
    const run: ReconciliationRun = {
      id: `run-${nextId()}`, tenantId, name: input.name,
      sourceAId: input.sourceAId, sourceBId: input.sourceBId, status: "completed",
      startedAt: "2026-05-25T00:00:00Z", completedAt: "2026-05-25T00:00:00Z", configVersion: "v1.0",
      stats: { matched: 0, unmatched: 0, partial: 0, duplicate: 0, breakCount: 0, matchRatePct: 0, valueAtRiskMinor: 0 },
    };
    this.state.runs.push(deepClone(run));
    return run;
  }

  // -------------------------------------------------------------------------
  // appendCaseEvent
  // -------------------------------------------------------------------------

  async appendCaseEvent(
    tenantId: string,
    caseId: string,
    event: NewCaseEvent
  ): Promise<Case> {
    await this.delay();

    const brk = this.state.breaks.find(
      (b) => b.caseId === caseId && b.tenantId === tenantId
    );
    if (!brk) {
      throw new Error(
        `Case "${caseId}" not found for tenant "${tenantId}".`
      );
    }

    const caseIdx = this.state.cases.findIndex((c) => c.id === caseId);
    if (caseIdx === -1) {
      throw new Error(`Case "${caseId}" data not found.`);
    }

    const c = this.state.cases[caseIdx];

    // Re-assembling a discriminated union after an object spread requires a
    // cast: TS cannot correlate the spread `kind` with its `payload` member.
    // Safe here because `event` is already a valid NewCaseEvent and we only add
    // server-assigned `id`/`at`.
    const newEvent = {
      ...event,
      id: nextId(),
      at: new Date().toISOString(),
    } as CaseEvent;

    // Determine status transition for approval-related events
    let updatedCase: Case;

    if (event.kind === "assignment") {
      const { assigneeId } = event.payload as { assigneeId: string };
      // Update the case
      updatedCase = {
        ...c,
        assigneeId,
        status: c.status === "open" ? "investigating" : c.status,
        events: [...c.events, newEvent],
      };
      // Also update the linked break
      const brkIdx = this.state.breaks.findIndex(
        (b) => b.caseId === caseId && b.tenantId === tenantId
      );
      if (brkIdx !== -1) {
        const linkedBrk = this.state.breaks[brkIdx];
        this.state.breaks[brkIdx] = {
          ...linkedBrk,
          assigneeId,
          status: linkedBrk.status === "open" ? "investigating" : linkedBrk.status,
        };
      }
    } else if (event.kind === "approval_requested") {
      // Find the actor user record for requestApproval
      const user = this.state.users.find((u) => u.id === event.actorId);
      if (!user) throw new Error(`User "${event.actorId}" not found.`);
      const payload = event.payload as { resolution: "write_off" | "manual_match" };
      updatedCase = requestApproval(c, user, payload.resolution);
      // Override the event id/at with our generated ones
      const lastEvent = updatedCase.events[updatedCase.events.length - 1];
      updatedCase = {
        ...updatedCase,
        events: [
          ...updatedCase.events.slice(0, -1),
          { ...lastEvent, id: newEvent.id, at: newEvent.at },
        ],
      };
    } else if (event.kind === "approved") {
      const user = this.state.users.find((u) => u.id === event.actorId);
      if (!user) throw new Error(`User "${event.actorId}" not found.`);
      updatedCase = approve(c, user);
      const lastEvent = updatedCase.events[updatedCase.events.length - 1];
      updatedCase = {
        ...updatedCase,
        events: [
          ...updatedCase.events.slice(0, -1),
          { ...lastEvent, id: newEvent.id, at: newEvent.at },
        ],
      };
    } else if (event.kind === "rejected") {
      const user = this.state.users.find((u) => u.id === event.actorId);
      if (!user) throw new Error(`User "${event.actorId}" not found.`);
      const payload = event.payload as { reason: string };
      updatedCase = reject(c, user, payload.reason);
      const lastEvent = updatedCase.events[updatedCase.events.length - 1];
      updatedCase = {
        ...updatedCase,
        events: [
          ...updatedCase.events.slice(0, -1),
          { ...lastEvent, id: newEvent.id, at: newEvent.at },
        ],
      };
    } else {
      // Generic event — just append, keep status
      updatedCase = {
        ...c,
        events: [...c.events, newEvent],
      };
    }

    this.state.cases[caseIdx] = updatedCase;

    return deepClone(this.state.cases[caseIdx]);
  }

  // -------------------------------------------------------------------------
  // Audit / Anchors / Controls
  // -------------------------------------------------------------------------

  async listAudit(tenantId: string, q?: AuditQuery): Promise<AuditPage> {
    await this.delay();
    let items = this.auditEvents.filter((e) => e.tenantId === tenantId);
    if (q?.from) items = items.filter((e) => e.at >= q.from!);
    if (q?.to) items = items.filter((e) => e.at <= q.to!);
    if (q?.kind && q.kind.length > 0) {
      const set = new Set(q.kind);
      items = items.filter((e) => set.has(e.kind));
    }
    if (q?.actorId) items = items.filter((e) => e.actorId === q.actorId);
    items = items.slice().sort((a, b) => b.seq - a.seq);
    if (typeof q?.before === "number") items = items.filter((e) => e.seq < q.before!);
    const limit = q?.limit ?? items.length;
    const page = items.slice(0, limit);
    const nextCursor = page.length === limit && items.length > limit ? page[page.length - 1].seq : null;
    return deepClone({ items: page, nextCursor });
  }

  async verifyAudit(
    tenantId: string, // eslint-disable-line @typescript-eslint/no-unused-vars
    body: VerifyRequest // eslint-disable-line @typescript-eslint/no-unused-vars
  ): Promise<VerifyResult> {
    await this.delay();
    const checked = this.auditEvents.filter((e) => e.tenantId === tenantId).length;
    return { status: "valid", checked };
  }

  async anchorAudit(
    tenantId: string // eslint-disable-line @typescript-eslint/no-unused-vars
  ): Promise<{ anchorSeq: number; hash: string }> {
    await this.delay();
    return { anchorSeq: 1, hash: "0".repeat(64) };
  }

  async listAnchors(
    tenantId: string, // eslint-disable-line @typescript-eslint/no-unused-vars
    limit?: number
  ): Promise<Anchor[]> {
    await this.delay();
    const anchors: Anchor[] = [
      {
        anchorSeq: 1,
        at: "2026-05-25T00:00:00Z",
        tenantHeads: {
          "tenant-acme": { seq: 3, hash: "c".repeat(64) },
        },
        prevHash: "0".repeat(64),
        hash: "d".repeat(64),
      },
      {
        anchorSeq: 2,
        at: "2026-05-26T00:00:00Z",
        tenantHeads: {
          "tenant-acme": { seq: 3, hash: "c".repeat(64) },
        },
        prevHash: "d".repeat(64),
        hash: "e".repeat(64),
      },
    ];
    return deepClone(typeof limit === "number" ? anchors.slice(0, limit) : anchors);
  }

  async listControls(): Promise<Control[]> {
    await this.delay();
    return deepClone([
      {
        id: "ISO27001:A.9.4.2",
        framework: "ISO 27001",
        description: "Secure log-on procedures",
        eventKinds: ["auth.login.success", "auth.login.failure", "auth.lockout"],
      },
      {
        id: "SOC2:CC6.1",
        framework: "SOC 2",
        description: "Logical access security",
        eventKinds: ["auth.login.success", "auth.login.failure", "auth.lockout"],
      },
      {
        id: "FCA:SYSC9.1",
        framework: "FCA",
        description: "Record keeping",
        eventKinds: ["data.ingest.completed", "data.run.created"],
      },
    ]);
  }
}
