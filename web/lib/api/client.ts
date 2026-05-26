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
} from "@/lib/domain/types";

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
export interface CreateSourceInput { kind: SourceKind; name: string; currency: string }
export type IngestFormat = "csv" | "camt053";
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
  listSources(tenantId: string): Promise<SourceListItem[]>;
  createSource(tenantId: string, input: CreateSourceInput): Promise<Source>;
  ingestFile(tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping): Promise<IngestResult>;
  createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun>;
}
