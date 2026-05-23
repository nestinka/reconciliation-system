import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { KpiCard } from "./kpi-card";

describe("KpiCard", () => {
  it("renders the label", () => {
    render(<KpiCard label="Total Breaks" value="42" />);
    expect(screen.getByText("Total Breaks")).toBeInTheDocument();
  });

  it("renders the value", () => {
    render(<KpiCard label="Total Breaks" value="42" />);
    expect(screen.getByText("42")).toBeInTheDocument();
  });

  it("renders a ReactNode value", () => {
    render(<KpiCard label="Amount" value={<span>$1,234.56</span>} />);
    expect(screen.getByText("$1,234.56")).toBeInTheDocument();
  });

  it("renders optional hint", () => {
    render(<KpiCard label="Count" value="10" hint="vs last week" />);
    expect(screen.getByText("vs last week")).toBeInTheDocument();
  });

  it("renders a down-delta with its textual value (not color-only)", () => {
    render(
      <KpiCard
        label="Breaks"
        value="5"
        delta={{ value: "-12%", direction: "down" }}
      />
    );
    // The delta numeric value must be visible as text
    expect(screen.getByText("-12%")).toBeInTheDocument();
    // And the sr-only direction label is also present
    expect(screen.getByText("(down)")).toBeInTheDocument();
  });

  it("renders an up-delta with direction label", () => {
    render(
      <KpiCard
        label="Resolved"
        value="20"
        delta={{ value: "+5%", direction: "up" }}
      />
    );
    expect(screen.getByText("+5%")).toBeInTheDocument();
    expect(screen.getByText("(up)")).toBeInTheDocument();
  });

  it("renders a flat-delta with direction label", () => {
    render(
      <KpiCard
        label="Open"
        value="8"
        delta={{ value: "0%", direction: "flat" }}
      />
    );
    expect(screen.getByText("0%")).toBeInTheDocument();
    expect(screen.getByText("(flat)")).toBeInTheDocument();
  });
});
