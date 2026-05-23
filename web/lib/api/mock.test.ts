import { describe, it, expect } from "vitest";
import { MockApiClient } from "./mock";

const api = () => new MockApiClient({ latencyMs: 0 });

describe("MockApiClient", () => {
  it("lists both tenants", async () => {
    expect((await api().listTenants()).length).toBe(2);
  });
  it("scopes runs by tenant", async () => {
    const c = api();
    const runs = await c.listRuns("tenant-acme");
    expect(runs.length).toBeGreaterThan(0);
    expect(runs.every((r) => r.tenantId === "tenant-acme")).toBe(true);
  });
  it("filters breaks by status", async () => {
    const c = api();
    const open = await c.listBreaks("tenant-acme", { status: "open" });
    expect(open.length).toBeGreaterThan(0);
    expect(open.every((b) => b.status === "open")).toBe(true);
  });
  it("getCase returns the pending case with its break and suggestions", async () => {
    const c = api();
    const res = await c.getCase("tenant-acme", "case-pending");
    expect(res.case.status).toBe("pending_approval");
    expect(res.brk.id).toBe("break-pending");
    expect(Array.isArray(res.suggestions)).toBe(true);
  });
  it("appendCaseEvent is append-only and does not mutate prior reads", async () => {
    const c = api();
    const before = (await c.getCase("tenant-acme", "case-pending")).case;
    const beforeLen = before.events.length;
    const next = await c.appendCaseEvent("tenant-acme", "case-pending", {
      kind: "comment",
      actorId: "user-sam",
      payload: { text: "looking into it" },
    });
    expect(next.events.length).toBe(beforeLen + 1);
    expect(before.events.length).toBe(beforeLen); // earlier read not mutated
  });
  it("assignBreak moves an open break to investigating", async () => {
    const c = api();
    const [open] = await c.listBreaks("tenant-acme", { status: "open" });
    const updated = await c.assignBreak("tenant-acme", open.id, "user-sam");
    expect(updated.assigneeId).toBe("user-sam");
    expect(updated.status).toBe("investigating");
  });
  it("assignBreak preserves status for a non-open break", async () => {
    const c = api();
    const updated = await c.assignBreak("tenant-acme", "break-pending", "user-sam");
    expect(updated.assigneeId).toBe("user-sam");
    expect(updated.status).toBe("pending_approval"); // not regressed to investigating
  });
});
