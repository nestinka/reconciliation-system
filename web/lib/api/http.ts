import type {
  ApiClient, BreakQuery, DashboardSummary, MatchSuggestion, NewCaseEvent, RunDetail, RunQuery,
} from "./client";
import type {
  Break, Case, CanonicalTransaction, ReconciliationRun, Tenant, User,
} from "@/lib/domain/types";
import { getAccessToken, runRefresh } from "@/lib/auth/token-store";

export class HttpApiClient implements ApiClient {
  constructor(private readonly baseUrl: string) {}

  private async req<T>(path: string, _tenantId: string | null, init?: RequestInit): Promise<T> {
    const headers: Record<string, string> = { ...(init?.headers as Record<string, string>) };
    const token = getAccessToken();
    if (token) headers["Authorization"] = `Bearer ${token}`;
    if (init?.body) headers["Content-Type"] = "application/json";

    const res = await fetch(`${this.baseUrl}${path}`, { ...init, headers });

    if (res.status === 401) {
      // Attempt a silent refresh once
      const newToken = await runRefresh();
      if (newToken) {
        const retryHeaders: Record<string, string> = { ...(init?.headers as Record<string, string>) };
        retryHeaders["Authorization"] = `Bearer ${newToken}`;
        if (init?.body) retryHeaders["Content-Type"] = "application/json";
        const retryRes = await fetch(`${this.baseUrl}${path}`, { ...init, headers: retryHeaders });
        if (!retryRes.ok) {
          let detail = `${retryRes.status}`;
          try { const b = await retryRes.json(); detail = b?.error?.code ?? b?.error?.message ?? detail; } catch { /* ignore */ }
          throw new Error(`API ${retryRes.status}: ${detail}`);
        }
        return retryRes.json() as Promise<T>;
      }
      // Refresh failed — throw the original 401
      let detail = "401";
      try { const b = await res.clone().json(); detail = b?.error?.code ?? b?.error?.message ?? detail; } catch { /* ignore */ }
      throw new Error(`API 401: ${detail}`);
    }

    if (!res.ok) {
      let detail = `${res.status}`;
      try { const b = await res.json(); detail = b?.error?.code ?? b?.error?.message ?? detail; } catch { /* ignore */ }
      throw new Error(`API ${res.status}: ${detail}`);
    }
    return res.json() as Promise<T>;
  }

  private qs(params: Record<string, string | undefined>): string {
    const sp = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v) sp.set(k, v);
    const s = sp.toString();
    return s ? `?${s}` : "";
  }

  listTenants(): Promise<Tenant[]> { return this.req("/api/tenants", null); }
  listUsers(tenantId: string): Promise<User[]> { return this.req("/api/users", tenantId); }
  getDashboard(tenantId: string): Promise<DashboardSummary> { return this.req("/api/dashboard", tenantId); }
  listRuns(tenantId: string, q?: RunQuery): Promise<ReconciliationRun[]> {
    return this.req(`/api/runs${this.qs({ status: q?.status, sourceId: q?.sourceId, from: q?.from, to: q?.to })}`, tenantId);
  }
  getRun(tenantId: string, runId: string): Promise<RunDetail> { return this.req(`/api/runs/${runId}`, tenantId); }
  listBreaks(tenantId: string, q?: BreakQuery): Promise<Break[]> {
    return this.req(`/api/breaks${this.qs({ status: q?.status, type: q?.type, ageingBucket: q?.ageingBucket, assigneeId: q?.assigneeId })}`, tenantId);
  }
  getCase(tenantId: string, caseId: string): Promise<{ case: Case; brk: Break; suggestions: MatchSuggestion[]; transactionsById: Record<string, CanonicalTransaction>; }> {
    return this.req(`/api/cases/${caseId}`, tenantId);
  }
  assignBreak(tenantId: string, breakId: string, userId: string): Promise<Break> {
    return this.req(`/api/breaks/${breakId}/assign`, tenantId, { method: "POST", body: JSON.stringify({ userId }) });
  }
  appendCaseEvent(tenantId: string, caseId: string, event: NewCaseEvent): Promise<Case> {
    return this.req(`/api/cases/${caseId}/events`, tenantId, { method: "POST", body: JSON.stringify(event) });
  }
}
