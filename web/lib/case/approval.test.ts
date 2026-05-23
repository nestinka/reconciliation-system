import { describe, it, expect } from "vitest";
import { canApprove, requestApproval, approve, reject } from "./approval";
import type { Case, User } from "@/lib/domain/types";

const maker: User = { id: "u1", name: "Mia", role: "operator" };
const checker: User = { id: "u2", name: "Theo", role: "approver" };

const pendingCase = (): Case => ({
  id: "c1", breakId: "b1", assigneeId: "u1", status: "pending_approval",
  events: [{ id: "e1", kind: "approval_requested", actorId: "u1", at: "2026-05-23T10:00:00Z", payload: { resolution: "write_off" } }],
});

describe("four-eyes", () => {
  it("rejects the maker approving their own proposal", () => {
    expect(canApprove(pendingCase(), maker).allowed).toBe(false);
  });
  it("allows a different approver to approve", () => {
    expect(canApprove(pendingCase(), checker).allowed).toBe(true);
  });
  it("blocks approval when not pending", () => {
    const c = { ...pendingCase(), status: "open" as const };
    expect(canApprove(c, checker).allowed).toBe(false);
  });
  it("requestApproval appends an approval_requested event and sets pending", () => {
    const open: Case = { id: "c1", breakId: "b1", assigneeId: "u1", status: "investigating", events: [] };
    const next = requestApproval(open, maker, "write_off");
    expect(next.status).toBe("pending_approval");
    expect(next.events.at(-1)?.kind).toBe("approval_requested");
    expect(next.events.length).toBe(open.events.length + 1);
    expect(open.events.length).toBe(0); // input not mutated
  });
  it("approve appends approved event and resolves", () => {
    const next = approve(pendingCase(), checker);
    expect(next.status).toBe("resolved");
    expect(next.events.at(-1)?.kind).toBe("approved");
  });
  it("reject appends rejected event and returns to investigating", () => {
    const next = reject(pendingCase(), checker, "needs more info");
    expect(next.status).toBe("investigating");
    expect(next.events.at(-1)?.kind).toBe("rejected");
  });
});
