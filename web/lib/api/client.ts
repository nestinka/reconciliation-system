import type {
  Tenant,
  User,
  UserRole,
  ReconciliationRun,
  RunStatus,
  MatchDecision,
  Break,
  BreakType,
  BreakStatus,
  AgeingBucket,
  Case,
  CaseEvent,
  CanonicalTransaction,
  Source,
  SourceKind,
  FormatDialect,
} from "@/lib/domain/types";

// Re-export FormatDialect so callers can import it from the API surface.
export type { FormatDialect } from "@/lib/domain/types";

export interface DashboardSummary {
  matchRatePct: number;
  openBreaks: number;
  valueAtRiskMinor: number;
  currency: string;
  slaAdherencePct: number;
  breaksByType: { type: BreakType; count: number }[];
  breaksByAgeing: { bucket: AgeingBucket; count: number }[];
  recentRuns: ReconciliationRun[];
}

export interface RunDetail {
  run: ReconciliationRun;
  transactionsById: Record<string, CanonicalTransaction>;
  matched: MatchDecision[];
  partial: MatchDecision[];
  duplicates: MatchDecision[];
  unmatched: Break[];
}

export interface MatchSuggestion {
  id: string;
  txnIds: string[];
  score: number;
  rationale: string;
}

export interface RunQuery {
  status?: RunStatus;
  sourceId?: string;
  from?: string;
  to?: string;
}

export interface BreakQuery {
  status?: BreakStatus;
  type?: BreakType;
  ageingBucket?: AgeingBucket;
  assigneeId?: string;
}

export type NewCaseEvent = Omit<CaseEvent, "id" | "at">;

export interface CreateUserInput {
  name: string;
  email: string;
  role: UserRole;
  password: string;
}

export interface UpdateUserPatch {
  role?: UserRole;
  disabled?: boolean;
}

export interface SourceListItem extends Source { txnCount: number }
export interface CreateSourceInput {
  kind: SourceKind;
  name: string;
  currency: string;
  // Optional on input; defaults to null when omitted. Only meaningful for bank
  // sources ingested via MT940 ("generic" | "subfielded") — other formats leave
  // it null.
  formatDialect?: FormatDialect | null;
  // Optional per-source PDF profile name (validated server-side against the registry).
  pdfProfile?: string | null;
}

export interface UpdateSourceInput {
  name?: string;
  // null = clear; undefined = don't change; string = set.
  formatDialect?: FormatDialect | null;
  // null = clear; undefined = don't change; string = set.
  pdfProfile?: string | null;
}
export type IngestFormat = "csv" | "camt053" | "mt940" | "mt942" | "bai2" | "pdf" | "auto";
export interface IngestResult { ingested: number; sourceId: string }
export interface CreateRunInput { name: string; sourceAId: string; sourceBId: string; from: string; to: string }

// CSV mapping mirrors the Rust serde shape exactly.
export type ColRef = { index: number } | { header: string };
export type AmountMapping =
  | { signed: { column: ColRef; debitWhenNegative: boolean } }
  | { debitCredit: { debit: ColRef; credit: ColRef } };
export interface CsvMapping {
  hasHeader: boolean;
  delimiter: number;            // byte value, 44 = ','
  externalRef: ColRef;
  valueDate: ColRef;
  dateFormat: string;
  amount: AmountMapping;
  description: ColRef;
  currency?: ColRef;
  counterparty?: ColRef;
}

// Audit chain types — wire shape matches backend recon-audit::AuditKind::as_str
export type AuditKind =
  | "auth.login.success" | "auth.login.failure" | "auth.lockout" | "auth.logout"
  | "auth.password.changed" | "auth.password.reset_requested" | "auth.password.reset_completed"
  | "auth.refresh.reused" | "auth.tenant.switched"
  | "admin.user.created" | "admin.user.role_changed" | "admin.user.disabled" | "admin.user.enabled" | "admin.user.removed"
  | "data.source.created" | "data.source.updated" | "data.ingest.completed" | "data.run.created"
  | "case.assigned" | "case.event_appended"
  | "system.anchor.created";

export interface AuditEvent {
  tenantId: string;
  seq: number;
  at: string;
  actorId: string;
  kind: AuditKind;
  payload: Record<string, unknown>;
  prevHash: string;  // hex
  hash: string;      // hex
}

export interface AuditPage { items: AuditEvent[]; nextCursor: number | null; }

export interface AuditQuery {
  from?: string; to?: string;
  kind?: AuditKind[]; actorId?: string;
  limit?: number; before?: number;
}

export interface VerifyRequest { from?: number; to?: number; expectedPrevHash?: string; }
export interface VerifyResult {
  status: "valid" | "invalid";
  checked: number;
  firstBrokenSeq?: number;
  reason?: "tampered" | "wrong_prev" | "missing" | "reordered" | "wrong_genesis";
}

export interface Anchor {
  anchorSeq: number;
  at: string;
  tenantHeads: Record<string, { seq: number; hash: string }>;
  prevHash: string;
  hash: string;
}

export interface Control {
  id: string;
  framework: string;
  description: string;
  eventKinds: AuditKind[];
}

export class IngestError extends Error {
  constructor(
    public code: "parse" | "duplicate",
    message: string,
    public rows?: { row: number; field: string; message: string }[],
    public refs?: string[],
  ) {
    super(message);
    this.name = "IngestError";
  }
}

export interface ApiClient {
  listTenants(): Promise<Tenant[]>;
  listUsers(tenantId: string): Promise<User[]>;
  listMembers(tenantId: string): Promise<User[]>;
  createUser(tenantId: string, input: CreateUserInput): Promise<User>;
  updateUser(tenantId: string, userId: string, patch: UpdateUserPatch): Promise<void>;
  deleteUser(tenantId: string, userId: string): Promise<void>;
  getDashboard(tenantId: string): Promise<DashboardSummary>;
  listRuns(tenantId: string, q?: RunQuery): Promise<ReconciliationRun[]>;
  getRun(tenantId: string, runId: string): Promise<RunDetail>;
  listBreaks(tenantId: string, q?: BreakQuery): Promise<Break[]>;
  getCase(
    tenantId: string,
    caseId: string
  ): Promise<{
    case: Case;
    brk: Break;
    suggestions: MatchSuggestion[];
    transactionsById: Record<string, CanonicalTransaction>;
  }>;
  assignBreak(
    tenantId: string,
    breakId: string,
    userId: string
  ): Promise<Break>;
  appendCaseEvent(
    tenantId: string,
    caseId: string,
    event: NewCaseEvent
  ): Promise<Case>;
  listSources(tenantId: string, includeArchived?: boolean): Promise<SourceListItem[]>;
  listPdfProfiles(tenantId: string): Promise<string[]>;
  createSource(tenantId: string, input: CreateSourceInput): Promise<Source>;
  updateSource(
    tenantId: string,
    sourceId: string,
    patch: UpdateSourceInput,
  ): Promise<Source>;
  archiveSource(tenantId: string, sourceId: string): Promise<void>;
  restoreSource(tenantId: string, sourceId: string): Promise<void>;
  ingestFile(tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping, dialect?: string, pdfProfile?: string): Promise<IngestResult>;
  createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun>;
  listAudit(tenantId: string, q?: AuditQuery): Promise<AuditPage>;
  verifyAudit(tenantId: string, body: VerifyRequest): Promise<VerifyResult>;
  anchorAudit(tenantId: string): Promise<{ anchorSeq: number; hash: string }>;
  listAnchors(tenantId: string, limit?: number): Promise<Anchor[]>;
  listControls(): Promise<Control[]>;
}
