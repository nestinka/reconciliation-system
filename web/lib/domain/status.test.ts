import { describe, it, expect } from "vitest";
import { statusMeta, type StatusKind } from "./status";

const ALL_STATUSES: StatusKind[] = [
  "matched",
  "partial",
  "unmatched",
  "break",
  "duplicate",
  "open",
  "investigating",
  "pending_approval",
  "resolved",
  "written_off",
  "running",
  "completed",
  "failed",
];

describe("statusMeta", () => {
  it("maps matched to success with a label and icon", () => {
    const m = statusMeta("matched");
    expect(m.tone).toBe("success");
    expect(m.label).toBe("Matched");
    expect(m.icon).toBeTruthy();
  });
  it("maps break to danger", () => {
    expect(statusMeta("break").tone).toBe("danger");
  });
  it("never relies on color alone (every status has a label)", () => {
    for (const s of ALL_STATUSES) {
      expect(statusMeta(s).label.length).toBeGreaterThan(0);
    }
  });
  it("provides an icon for every status", () => {
    for (const s of ALL_STATUSES) {
      expect(statusMeta(s).icon).toBeTruthy();
    }
  });
});
