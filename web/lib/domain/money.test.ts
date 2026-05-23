import { describe, it, expect } from "vitest";
import { formatMoney } from "./money";

describe("formatMoney", () => {
  it("formats minor units with currency", () => {
    expect(formatMoney(123456, "GBP")).toBe("£1,234.56");
  });
  it("handles zero-decimal currencies", () => {
    expect(formatMoney(1000, "JPY")).toBe("¥1,000");
  });
  it("formats negatives", () => {
    expect(formatMoney(-5000, "USD")).toBe("-$50.00");
  });
});
