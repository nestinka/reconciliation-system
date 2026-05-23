import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { axe } from "jest-axe";
import { StatusPill } from "./status-pill";

describe("StatusPill", () => {
  it("renders the label for 'break'", () => {
    render(<StatusPill status="break" />);
    expect(screen.getByText("Break")).toBeInTheDocument();
  });

  it("renders the label for 'matched'", () => {
    render(<StatusPill status="matched" />);
    expect(screen.getByText("Matched")).toBeInTheDocument();
  });

  it("renders the label for 'pending_approval'", () => {
    render(<StatusPill status="pending_approval" />);
    expect(screen.getByText("Pending Approval")).toBeInTheDocument();
  });

  it("renders the label for 'investigating'", () => {
    render(<StatusPill status="investigating" />);
    expect(screen.getByText("Investigating")).toBeInTheDocument();
  });

  it("renders the label for 'failed'", () => {
    render(<StatusPill status="failed" />);
    expect(screen.getByText("Failed")).toBeInTheDocument();
  });

  it("passes axe accessibility check", async () => {
    const { container } = render(<StatusPill status="partial" />);
    const results = await axe(container);
    expect(results).toHaveNoViolations();
  });
});
