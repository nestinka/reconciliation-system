import type {
  Tenant,
  User,
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

export interface ApiClient {
  listTenants(): Promise<Tenant[]>;
  listUsers(tenantId: string): Promise<User[]>;
  getDashboard(tenantId: string): Promise<DashboardSummary>;
  listRuns(tenantId: string, q?: RunQuery): Promise<ReconciliationRun[]>;
  getRun(tenantId: string, runId: string): Promise<RunDetail>;
  listBreaks(tenantId: string, q?: BreakQuery): Promise<Break[]>;
  getCase(
    tenantId: string,
    caseId: string
  ): Promise<{ case: Case; brk: Break; suggestions: MatchSuggestion[] }>;
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
}
