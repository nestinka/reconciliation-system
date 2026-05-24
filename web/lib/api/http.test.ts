import { describe, it, expect, vi, beforeEach } from "vitest";
import { HttpApiClient } from "./http";

const okJson = (body: unknown) =>
  Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) } as Response);

describe("HttpApiClient", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("sends X-Tenant-Id and parses dashboard", async () => {
    const fetchMock = vi.fn<typeof fetch>(() => okJson({ matchRatePct: 91.2, openBreaks: 3 }));
    vi.stubGlobal("fetch", fetchMock);
    const c = new HttpApiClient("http://api.test");
    const d = await c.getDashboard("tenant-acme");
    expect(d.openBreaks).toBe(3);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("http://api.test/api/dashboard");
    expect((init as RequestInit).headers).toMatchObject({ "X-Tenant-Id": "tenant-acme" });
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
});
