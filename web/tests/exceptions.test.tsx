import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderWithProviders, screen, waitFor, userEvent } from "./test-utils";
import ExceptionsPage from "@/app/(app)/exceptions/page";

// next/navigation useRouter is not available in jsdom — mock it.
// (nuqs uses NuqsTestingAdapter from test-utils, so useSearchParams is handled.)
const mockPush = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: mockPush }),
}));

describe("ExceptionsPage (breaks/exceptions list)", () => {
  beforeEach(() => {
    mockPush.mockClear();
  });

  it("renders the page header", () => {
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });
    expect(screen.getByText("Exceptions")).toBeInTheDocument();
  });

  it("renders break rows after data loads", async () => {
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });

    // Wait for the table to populate — status pills render capitalized labels
    // e.g. "Open", "Investigating", "Pending Approval"
    await waitFor(() => {
      // The status column should show at least one "Open" pill
      const openCells = screen.getAllByText("Open");
      expect(openCells.length).toBeGreaterThan(0);
    });

    // Assignee names appear for assigned breaks (Sam, Mia, Theo, Ada)
    // break-002 and break-014 both have user-sam; break-pending and break-010 have user-mia
    expect(screen.getAllByText("Sam").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Mia").length).toBeGreaterThan(0);
  });

  it("shows loading skeletons before data resolves", () => {
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });
    // The 0-latency mock resolves on a microtask, so the synchronous render is loading
    expect(document.querySelector("[data-slot='skeleton']")).toBeTruthy();
  });

  it("filter via initial searchParams: type=duplicate shows only duplicate breaks", async () => {
    renderWithProviders(<ExceptionsPage />, {
      tenantId: "tenant-acme",
      searchParams: "?type=duplicate",
    });

    // Wait for data to load — duplicates include break-005 and break-012
    // StatusPill renders capitalized labels: "Duplicate"
    await waitFor(() => {
      const duplicatePills = screen.getAllByText("Duplicate");
      expect(duplicatePills.length).toBeGreaterThan(0);
    });

    // break-001 is type=unmatched — "Unmatched" pill should NOT be visible
    expect(screen.queryByText("Unmatched")).not.toBeInTheDocument();

    // break-002 is type=partial — "Partial" pill should NOT appear
    expect(screen.queryByText("Partial")).not.toBeInTheDocument();
  });

  it("selection toolbar: selecting a row shows '1 selected'", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });

    // Wait for rows to load
    await waitFor(() => {
      expect(screen.getAllByText("Open").length).toBeGreaterThan(0);
    });

    // Find the first row checkbox (not the header checkbox)
    // The header checkbox is "Select all rows"; row checkboxes are "Select break ..."
    const rowCheckboxes = screen
      .getAllByRole("checkbox")
      .filter((cb) =>
        cb.getAttribute("aria-label")?.startsWith("Select break ")
      );
    expect(rowCheckboxes.length).toBeGreaterThan(0);

    await user.click(rowCheckboxes[0]);

    // Toolbar should appear with "1 selected"
    await waitFor(() => {
      expect(screen.getByText("1 selected")).toBeInTheDocument();
    });
  });

  it("selection toolbar: clicking Clear hides the toolbar", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(screen.getAllByText("Open").length).toBeGreaterThan(0);
    });

    const rowCheckboxes = screen
      .getAllByRole("checkbox")
      .filter((cb) =>
        cb.getAttribute("aria-label")?.startsWith("Select break ")
      );
    await user.click(rowCheckboxes[0]);

    await waitFor(() => {
      expect(screen.getByText("1 selected")).toBeInTheDocument();
    });

    // Click Clear
    const clearButton = screen.getByRole("button", { name: /clear/i });
    await user.click(clearButton);

    // Toolbar should disappear
    await waitFor(() => {
      expect(screen.queryByText("1 selected")).not.toBeInTheDocument();
    });
  });

  it("row navigation: clicking a row calls router.push with /cases/{caseId}", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(screen.getAllByText("Open").length).toBeGreaterThan(0);
    });

    // Find a table row that is clickable (has cursor-pointer class).
    // We target the first data row (not a skeleton row).
    // The DataTable renders rows as <tr> with tabIndex=0 when clickable.
    const rows = document
      .querySelectorAll("tbody tr[tabindex='0']");
    expect(rows.length).toBeGreaterThan(0);

    // Click the first clickable row
    await user.click(rows[0] as HTMLElement);

    expect(mockPush).toHaveBeenCalledWith(
      expect.stringMatching(/^\/cases\/case-/)
    );
  });

  it("renders filter selects", () => {
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });
    expect(
      screen.getByRole("combobox", { name: /filter by type/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: /filter by status/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: /filter by ageing/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("combobox", { name: /filter by assignee/i })
    ).toBeInTheDocument();
  });

  it("selecting multiple rows shows correct count in toolbar", async () => {
    const user = userEvent.setup();
    renderWithProviders(<ExceptionsPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(screen.getAllByText("Open").length).toBeGreaterThan(0);
    });

    const rowCheckboxes = screen
      .getAllByRole("checkbox")
      .filter((cb) =>
        cb.getAttribute("aria-label")?.startsWith("Select break ")
      );

    // Select first two
    await user.click(rowCheckboxes[0]);
    await user.click(rowCheckboxes[1]);

    await waitFor(() => {
      expect(screen.getByText("2 selected")).toBeInTheDocument();
    });
  });
});
