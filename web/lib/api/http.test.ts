import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { HttpApiClient } from "./http";
import * as tokenStore from "@/lib/auth/token-store";

const okJson = (body: unknown) =>
  Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) } as Response);

describe("HttpApiClient", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    tokenStore.setAccessToken(null);
    tokenStore.registerRefresh(null);
  });

  afterEach(() => {
    tokenStore.setAccessToken(null);
    tokenStore.registerRefresh(null);
  });

  it("sends Authorization: Bearer when a token is set", async () => {
    tokenStore.setAccessToken("test-token-abc");
    const fetchMock = vi.fn<typeof fetch>(() => okJson({ matchRatePct: 91.2, openBreaks: 3 }));
    vi.stubGlobal("fetch", fetchMock);
    const c = new HttpApiClient("http://api.test");
    const d = await c.getDashboard("tenant-acme");
    expect(d.openBreaks).toBe(3);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("http://api.test/api/dashboard");
    expect((init as RequestInit).headers).toMatchObject({ "Authorization": "Bearer test-token-abc" });
  });

  it("does not send Authorization header when no token is set", async () => {
    const fetchMock = vi.fn<typeof fetch>(() => okJson({ matchRatePct: 91.2, openBreaks: 3 }));
    vi.stubGlobal("fetch", fetchMock);
    const c = new HttpApiClient("http://api.test");
    await c.getDashboard("tenant-acme");
    const [, init] = fetchMock.mock.calls[0];
    const headers = (init as RequestInit).headers as Record<string, string>;
    expect(headers["Authorization"]).toBeUndefined();
  });

  it("encodes break query params", async () => {
    const fetchMock = vi.fn<typeof fetch>(() => okJson([]));
    vi.stubGlobal("fetch", fetchMock);
    const c = new HttpApiClient("http://api.test");
    await c.listBreaks("tenant-acme", { status: "open", type: "duplicate" });
    expect(fetchMock.mock.calls[0][0]).toBe("http://api.test/api/breaks?status=open&type=duplicate");
  });

  it("throws on non-2xx", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: false, status: 403,
      json: () => Promise.resolve({ error: { code: "forbidden", message: "no" } }) } as Response)));
    const c = new HttpApiClient("http://api.test");
    await expect(c.appendCaseEvent("tenant-acme", "case-pending",
      { actorId: "user-mia", kind: "approved", payload: {} } as never)).rejects.toThrow(/forbidden/);
  });

  it("retries with new token after 401 if refresh succeeds", async () => {
    tokenStore.setAccessToken("old-token");
    tokenStore.registerRefresh(async () => {
      tokenStore.setAccessToken("new-token");
      return "new-token";
    });

    let callCount = 0;
    const fetchMock = vi.fn<typeof fetch>(() => {
      callCount++;
      if (callCount === 1) {
        return Promise.resolve({ ok: false, status: 401, clone: () => ({ json: () => Promise.resolve({}) }), json: () => Promise.resolve({}) } as unknown as Response);
      }
      return okJson({ matchRatePct: 95, openBreaks: 1 });
    });
    vi.stubGlobal("fetch", fetchMock);

    const c = new HttpApiClient("http://api.test");
    const result = await c.getDashboard("tenant-acme");
    expect(result.openBreaks).toBe(1);
    expect(fetchMock).toHaveBeenCalledTimes(2);

    // Second call should use the new token
    const [, retryInit] = fetchMock.mock.calls[1];
    expect((retryInit as RequestInit).headers).toMatchObject({ "Authorization": "Bearer new-token" });
  });

  it("throws after 401 if refresh returns null", async () => {
    tokenStore.setAccessToken("old-token");
    tokenStore.registerRefresh(async () => null);

    const fetchMock = vi.fn<typeof fetch>(() =>
      Promise.resolve({ ok: false, status: 401, clone: () => ({ json: () => Promise.resolve({}) }), json: () => Promise.resolve({}) } as unknown as Response)
    );
    vi.stubGlobal("fetch", fetchMock);

    const c = new HttpApiClient("http://api.test");
    await expect(c.getDashboard("tenant-acme")).rejects.toThrow(/401/);
  });
});
