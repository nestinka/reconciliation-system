import { describe, it, expect } from "vitest";
import { statusMeta } from "./status";

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
  it("never relies on color alone (always has a label)", () => {
    for (const s of ["matched","partial","unmatched","break","pending_approval","resolved","written_off"] as const) {
      expect(statusMeta(s).label.length).toBeGreaterThan(0);
    }
  });
});
