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
  it("getCase returns a non-empty transactionsById for case-pending", async () => {
    const c = api();
    const res = await c.getCase("tenant-acme", "case-pending");
    expect(typeof res.transactionsById).toBe("object");
    expect(Object.keys(res.transactionsById).length).toBeGreaterThan(0);
    // break-pending references txn-brk001 which should be in the map
    expect(res.transactionsById["txn-brk001"]).toBeDefined();
  });
  it("appendCaseEvent with assignment sets the case assigneeId", async () => {
    const c = api();
    // Start with case-001 (open, no assignee)
    const updated = await c.appendCaseEvent("tenant-acme", "case-001", {
      kind: "assignment",
      actorId: "user-ada",
      payload: { assigneeId: "user-theo" },
    });
    expect(updated.assigneeId).toBe("user-theo");
    // Verify the event was appended
    const lastEvent = updated.events[updated.events.length - 1];
    expect(lastEvent.kind).toBe("assignment");
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

  it("createSource then listSources includes it with a txn count", async () => {
    const c = new MockApiClient({ latencyMs: 0 });
    const before = (await c.listSources("tenant-acme")).length;
    const src = await c.createSource("tenant-acme", { kind: "bank", name: "New Bank", currency: "GBP" });
    const after = await c.listSources("tenant-acme");
    expect(after.length).toBe(before + 1);
    expect(after.find((s) => s.id === src.id)?.txnCount).toBe(0);
  });

  it("ingestFile records a transaction and returns a count", async () => {
    const c = new MockApiClient({ latencyMs: 0 });
    const src = await c.createSource("tenant-acme", { kind: "bank", name: "B", currency: "GBP" });
    const res = await c.ingestFile("tenant-acme", src.id, "csv", new File(["x"], "f.csv"), undefined);
    expect(res.ingested).toBe(1);
    expect((await c.listSources("tenant-acme")).find((s) => s.id === src.id)?.txnCount).toBe(1);
  });

  it("createRun appends a run", async () => {
    const c = new MockApiClient({ latencyMs: 0 });
    const run = await c.createRun("tenant-acme", { name: "R", sourceAId: "a", sourceBId: "b", from: "2026-05-01", to: "2026-05-31" });
    expect(run.id).toMatch(/^run-/);
  });

  it("listAudit returns items for tenant", async () => {
    const c = api();
    const page = await c.listAudit("tenant-acme");
    expect(page.items.length).toBeGreaterThan(0);
    expect(page.items.every((e) => e.tenantId === "tenant-acme")).toBe(true);
    // Sorted by seq desc
    for (let i = 1; i < page.items.length; i++) {
      expect(page.items[i - 1].seq).toBeGreaterThan(page.items[i].seq);
    }
  });

  it("verifyAudit returns valid", async () => {
    const c = api();
    const r = await c.verifyAudit("tenant-acme", {});
    expect(r.status).toBe("valid");
    expect(r.checked).toBeGreaterThan(0);
  });

  it("anchorAudit returns a seq", async () => {
    const c = api();
    const a = await c.anchorAudit("tenant-acme");
    expect(typeof a.anchorSeq).toBe("number");
    expect(a.hash).toMatch(/^[0-9a-f]{64}$/);
  });

  it("listAnchors returns array", async () => {
    const c = api();
    const anchors = await c.listAnchors("tenant-acme");
    expect(Array.isArray(anchors)).toBe(true);
    expect(anchors.length).toBeGreaterThan(0);
    expect(anchors[0].anchorSeq).toBeDefined();
  });

  it("listControls returns three frameworks", async () => {
    const c = api();
    const controls = await c.listControls();
    expect(controls.length).toBe(3);
    const frameworks = controls.map((x) => x.framework);
    expect(frameworks).toContain("ISO 27001");
    expect(frameworks).toContain("SOC 2");
    expect(frameworks).toContain("FCA");
  });

  it("createSource round-trips formatDialect", async () => {
    const c = new MockApiClient({ latencyMs: 0 });
    const src = await c.createSource("tenant-acme", {
      kind: "bank",
      name: "MT940 Acme",
      currency: "GBP",
      formatDialect: "subfielded",
    });
    expect(src.formatDialect).toBe("subfielded");
  });

  it("createSource without formatDialect defaults to null", async () => {
    const c = new MockApiClient({ latencyMs: 0 });
    const src = await c.createSource("tenant-acme", {
      kind: "bank",
      name: "Plain",
      currency: "GBP",
    });
    expect(src.formatDialect).toBeNull();
  });
});
