import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderWithProviders, screen, waitFor, userEvent } from "./test-utils";
import RunDetailPage from "@/app/(app)/runs/[runId]/page";

// Mock next/navigation useParams → run-acme-001 and useRouter
const mockPush = vi.fn();
vi.mock("next/navigation", () => ({
  useParams: () => ({ runId: "run-acme-001" }),
  useRouter: () => ({ push: mockPush }),
}));

// run-acme-001 fixture summary:
//   name: "Daily Bank-GL 2026-05-01"
//   status: "completed"
//   stats: { matched: 95, unmatched: 3, partial: 2, duplicate: 0, breakCount: 3, matchRatePct: 95.0, valueAtRiskMinor: 125000 }
//   matched decisions: md-001, md-002, md-003, md-004 (4 items in fixture)
//   partial decisions: md-005 (1 item)
//   duplicates: [] (none for this run)
//   unmatched (breaks): break-001 (caseId: "case-001"), break-009 (caseId: "case-009")

describe("RunDetailPage", () => {
  beforeEach(() => {
    mockPush.mockClear();
  });

  it("renders the run name in the page header after load", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(
        screen.getByRole("heading", { name: "Daily Bank-GL 2026-05-01" })
      ).toBeInTheDocument();
    });
  });

  it("renders a StatusPill for the run status (completed)", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // StatusPill renders the label text "Completed"
      expect(screen.getByText("Completed")).toBeInTheDocument();
    });
  });

  it("renders stats summary KPI cards after load", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // KPI labels appear as text; "Matched" also appears in StatusPills so use getAllByText
      expect(screen.getAllByText("Matched").length).toBeGreaterThan(0);
      expect(screen.getAllByText("Partial").length).toBeGreaterThan(0);
      expect(screen.getByText("Breaks")).toBeInTheDocument();
      expect(screen.getByText("Match rate")).toBeInTheDocument();
    });
    // run-acme-001 matchRatePct = 95.0 → "95.0%"
    expect(screen.getByText("95.0%")).toBeInTheDocument();
  });

  it("renders the value at risk formatted as currency", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // valueAtRiskMinor: 125000 GBP → £1,250.00
      expect(screen.getByText("£1,250.00")).toBeInTheDocument();
    });
  });

  it("renders tab triggers with counts", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // 4 matched decisions in fixture for run-acme-001
      expect(
        screen.getByRole("tab", { name: /matched \(4\)/i })
      ).toBeInTheDocument();
      // 1 partial decision
      expect(
        screen.getByRole("tab", { name: /partial \(1\)/i })
      ).toBeInTheDocument();
      // 0 duplicates
      expect(
        screen.getByRole("tab", { name: /duplicates \(0\)/i })
      ).toBeInTheDocument();
      // 2 unmatched breaks
      expect(
        screen.getByRole("tab", { name: /unmatched \(2\)/i })
      ).toBeInTheDocument();
    });
  });

  it("shows matched decision rows in the Matched tab (default)", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // md-001 references txn-a001 (BANK-20260501-001) and txn-b001 (GL-20260501-001)
      expect(screen.getByText(/BANK-20260501-001/)).toBeInTheDocument();
    });
  });

  it("switching to Unmatched tab shows break rows", async () => {
    const user = userEvent.setup();
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(
        screen.getByRole("tab", { name: /unmatched/i })
      ).toBeInTheDocument();
    });

    await user.click(screen.getByRole("tab", { name: /unmatched/i }));

    await waitFor(() => {
      // break-001 → txn-brk002 → externalRef: "BANK-20260516-010"
      expect(screen.getByText(/BANK-20260516-010/)).toBeInTheDocument();
    });
  });

  it("clicking an unmatched break row navigates to /cases/{caseId}", async () => {
    const user = userEvent.setup();
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(
        screen.getByRole("tab", { name: /unmatched/i })
      ).toBeInTheDocument();
    });

    await user.click(screen.getByRole("tab", { name: /unmatched/i }));

    await waitFor(() => {
      // break-001 → txn-brk002 externalRef is BANK-20260516-010
      expect(screen.getByText(/BANK-20260516-010/)).toBeInTheDocument();
    });

    // Click the row containing that reference
    await user.click(screen.getByText(/BANK-20260516-010/));

    // break-001 has caseId: "case-001"
    expect(mockPush).toHaveBeenCalledWith("/cases/case-001");
  });

  it("Duplicates tab shows empty state for run-acme-001 (no duplicates)", async () => {
    const user = userEvent.setup();
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(
        screen.getByRole("tab", { name: /duplicates/i })
      ).toBeInTheDocument();
    });

    await user.click(screen.getByRole("tab", { name: /duplicates/i }));

    await waitFor(() => {
      expect(screen.getByText("No duplicates")).toBeInTheDocument();
    });
  });

  it("shows loading skeleton while data is fetching", () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    // aria-busy skeleton container
    expect(
      document.querySelector("[aria-busy='true']")
    ).toBeTruthy();
    // Run name should not be visible yet
    expect(
      screen.queryByText("Daily Bank-GL 2026-05-01")
    ).not.toBeInTheDocument();
  });

  it("renders the config version after load", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // run-acme-001 configVersion: "v1.2", rendered as "Config: v1.2"
      expect(screen.getByText(/Config:\s*v1\.2/)).toBeInTheDocument();
    });
  });

  it("renders started date after load", async () => {
    renderWithProviders(<RunDetailPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      // startedAt: "2026-05-01T18:00:00Z" → "01 May 2026"
      // completedAt is also 2026-05-01 so multiple matches expected
      expect(screen.getAllByText(/01 May 2026/).length).toBeGreaterThan(0);
    });
    // "Started:" label should be present
    expect(screen.getByText(/Started:/)).toBeInTheDocument();
  });
});
