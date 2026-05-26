import type {
  ApiClient, BreakQuery, CreateUserInput, DashboardSummary, MatchSuggestion, NewCaseEvent, RunDetail, RunQuery, UpdateUserPatch, SourceListItem, CreateSourceInput, IngestFormat, IngestResult, CreateRunInput, CsvMapping,
} from "./client";
import { IngestError } from "./client";
import type {
  Break, Case, CanonicalTransaction, ReconciliationRun, Source, Tenant, User,
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
  listMembers(tenantId: string): Promise<User[]> { return this.req("/api/members", tenantId); }
  createUser(tenantId: string, input: CreateUserInput): Promise<User> {
    return this.req("/api/users", tenantId, { method: "POST", body: JSON.stringify(input) });
  }
  async updateUser(tenantId: string, userId: string, patch: UpdateUserPatch): Promise<void> {
    await this.req(`/api/users/${userId}`, tenantId, { method: "PATCH", body: JSON.stringify(patch) });
  }
  async deleteUser(tenantId: string, userId: string): Promise<void> {
    await this.req(`/api/users/${userId}`, tenantId, { method: "DELETE" });
  }
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

  listSources(tenantId: string): Promise<SourceListItem[]> { return this.req("/api/sources", tenantId); }
  createSource(tenantId: string, input: CreateSourceInput): Promise<Source> {
    return this.req("/api/sources", tenantId, { method: "POST", body: JSON.stringify(input) });
  }
  createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun> {
    return this.req("/api/runs", tenantId, { method: "POST", body: JSON.stringify(input) });
  }

  async ingestFile(_tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping): Promise<IngestResult> {
    const send = async (token: string | null): Promise<Response> => {
      const fd = new FormData();
      fd.append("file", file);
      fd.append("format", format);
      if (mapping) fd.append("mapping", JSON.stringify(mapping));
      const headers: Record<string, string> = {};
      if (token) headers["Authorization"] = `Bearer ${token}`;
      // NOTE: do not set Content-Type — the browser sets the multipart boundary.
      return fetch(`${this.baseUrl}/api/sources/${sourceId}/ingest`, { method: "POST", headers, body: fd });
    };

    let res = await send(getAccessToken());
    if (res.status === 401) {
      const newToken = await runRefresh();
      if (!newToken) throw new Error("API 401: unauthorized");
      res = await send(newToken);
    }
    if (res.ok) return res.json() as Promise<IngestResult>;

    // Structured ingest errors (422 parse / 409 duplicate).
    let body: { error?: { code?: string; message?: string; rows?: { row: number; field: string; message: string }[]; refs?: string[] } } = {};
    try { body = await res.json(); } catch { /* ignore */ }
    const err = body.error;
    if (err?.code === "parse") throw new IngestError("parse", err.message ?? "parse error", err.rows);
    if (err?.code === "duplicate") throw new IngestError("duplicate", err.message ?? "duplicate", undefined, err.refs);
    throw new Error(`API ${res.status}: ${err?.code ?? err?.message ?? res.status}`);
  }
}
