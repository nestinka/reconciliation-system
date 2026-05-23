import { describe, it, expect, vi } from "vitest";
import { axe } from "jest-axe";
import { renderWithProviders, screen, waitFor, userEvent } from "./test-utils";
import DashboardPage from "@/app/(app)/dashboard/page";

// next/navigation useRouter is not available in jsdom — mock it.
const mockPush = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: mockPush }),
}));

// The MockApiClient with latencyMs=0 returns data via resolved promises
// (microtasks), so waitFor resolves on the next tick.

describe("DashboardPage", () => {
  it("renders page header", () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    expect(screen.getByText("Dashboard")).toBeInTheDocument();
    expect(
      screen.getByText("Reconciliation health across your sources.")
    ).toBeInTheDocument();
  });

  it("renders KPI labels after load", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    // "Match rate" and "Value at risk" also appear as table column headers, so
    // use getAllByText (which doesn't throw on multiple) to confirm presence.
    await waitFor(() => {
      // "Open breaks" only appears as a KPI label
      expect(screen.getByText("Open breaks")).toBeInTheDocument();
      expect(screen.getByText("SLA adherence")).toBeInTheDocument();
      expect(screen.getAllByText("Match rate").length).toBeGreaterThan(0);
      expect(screen.getAllByText("Value at risk").length).toBeGreaterThan(0);
    });
  });

  it("renders KPI: match rate value (93.7% for tenant-acme)", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // avg of 6 completed runs: 95.0+97.1+91.7+96.5+89.8+92.3 = 562.4/6 ≈ 93.7
      expect(screen.getByText("93.7%")).toBeInTheDocument();
    });
  });

  it("renders KPI: open breaks count (13 for tenant-acme)", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // 13 open+investigating+pending_approval breaks for tenant-acme
      // Use getAllByText to avoid ambiguity with other numbers on the page
      const elements = screen.getAllByText("13");
      expect(elements.length).toBeGreaterThan(0);
    });
  });

  it("renders KPI: value at risk hint text after load", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // "sum across open breaks" is the hint text unique to the VAR KPI card
      expect(screen.getByText("sum across open breaks")).toBeInTheDocument();
    });
  });

  it("renders KPI: SLA adherence value (0.0% for tenant-acme)", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(screen.getByText("SLA adherence")).toBeInTheDocument();
    });
    // 0 of 2 resolved breaks had ageingDays <= 7
    expect(screen.getByText("0.0%")).toBeInTheDocument();
  });

  it("renders breaksByType with accessible labels and counts", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(screen.getByText("Breaks by type")).toBeInTheDocument();
    });
    // The accessible list (aria-label="Break counts by type") must show type names as text
    expect(screen.getByText("Unmatched")).toBeInTheDocument();
    expect(screen.getByText("Partial")).toBeInTheDocument();
    expect(screen.getByText("Duplicate")).toBeInTheDocument();
    // The "Break" type label appears in the list
    const list = screen.getByRole("list", { name: "Break counts by type" });
    expect(list).toBeInTheDocument();
    // Break type items have count text (numeric)
    const items = list.querySelectorAll("li");
    expect(items.length).toBe(4);
  });

  it("renders breaksByAgeing section with age bucket labels", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(screen.getByText("Break ageing")).toBeInTheDocument();
    });
    expect(screen.getByText("0–1 day")).toBeInTheDocument();
    expect(screen.getByText("2–7 days")).toBeInTheDocument();
    expect(screen.getByText("8–30 days")).toBeInTheDocument();
    expect(screen.getByText("30d+")).toBeInTheDocument();
  });

  it("renders recent runs table with known fixture run name", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // "Daily Bank-GL 2026-05-15" is the most recent completed run for tenant-acme
      expect(
        screen.getByText("Daily Bank-GL 2026-05-15")
      ).toBeInTheDocument();
    });
  });

  it("renders StatusPill for recent runs (completed)", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // All recent runs are completed — StatusPill renders "Completed" label text
      const pills = screen.getAllByText("Completed");
      expect(pills.length).toBeGreaterThan(0);
    });
  });

  it("renders recent runs heading", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: /recent runs/i })
      ).toBeInTheDocument();
    });
  });

  it("shows skeletons while loading (before data resolves)", () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    // The 0-latency mock resolves on a microtask, so the synchronous first
    // render is the loading state.
    expect(document.querySelector("[data-slot='skeleton']")).toBeTruthy();
    expect(screen.queryByText("Open breaks")).not.toBeInTheDocument();
  });

  it("formats the value-at-risk KPI as currency (not raw minor units)", async () => {
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(screen.getByText("sum across open breaks")).toBeInTheDocument();
    });
    // A formatMoney result like "£12,345.67" must appear; raw minor units would
    // have no currency symbol or decimal point.
    expect(screen.getAllByText(/£[\d,]+\.\d{2}/).length).toBeGreaterThan(0);
  });

  it("has no critical a11y violations after load", async () => {
    const { container } = renderWithProviders(<DashboardPage />, {
      tenantId: "tenant-acme",
    });
    // Wait for data to resolve (KPI labels visible)
    await waitFor(() => {
      expect(screen.getByText("Open breaks")).toBeInTheDocument();
    });
    const results = await axe(container, {
      runOnly: { type: "tag", values: ["wcag2a", "wcag2aa"] },
    });
    expect(results).toHaveNoViolations();
  });

  it("navigates to the run detail when a recent-run row is clicked", async () => {
    mockPush.mockClear();
    renderWithProviders(<DashboardPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(screen.getByText("Daily Bank-GL 2026-05-15")).toBeInTheDocument();
    });
    const user = userEvent.setup();
    await user.click(screen.getByText("Daily Bank-GL 2026-05-15"));
    expect(mockPush).toHaveBeenCalledWith(expect.stringMatching(/^\/runs\//));
  });
});
