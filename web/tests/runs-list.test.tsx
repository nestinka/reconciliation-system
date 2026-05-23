import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderWithProviders, screen, waitFor, userEvent } from "./test-utils";
import RunsPage from "@/app/(app)/runs/page";

// next/navigation useRouter is not available in jsdom — mock it.
const mockPush = vi.fn();
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: mockPush }),
  useSearchParams: () => new URLSearchParams(),
}));

describe("RunsPage (runs list)", () => {
  beforeEach(() => {
    mockPush.mockClear();
  });

  it("renders the page header", () => {
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });
    expect(screen.getByText("Reconciliation runs")).toBeInTheDocument();
  });

  it("renders known run names after data loads", async () => {
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });
    await waitFor(() => {
      expect(
        screen.getByText("Daily Bank-GL 2026-05-01")
      ).toBeInTheDocument();
    });
    // All 8 acme runs should be present (unfiltered)
    expect(
      screen.getByText("Daily Bank-GL 2026-05-23")
    ).toBeInTheDocument();
    expect(
      screen.getByText("Cross-System Recon 2026-05-10")
    ).toBeInTheDocument();
  });

  it("renders the status Select filter", async () => {
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });
    // The Select trigger should be present
    const trigger = screen.getByRole("combobox", { name: /filter by status/i });
    expect(trigger).toBeInTheDocument();
  });

  it("renders the name search Input", async () => {
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });
    const input = screen.getByRole("textbox", { name: /search runs by name/i });
    expect(input).toBeInTheDocument();
  });

  it("typing in the name filter narrows visible rows", async () => {
    const user = userEvent.setup();
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });

    // Wait for data to load
    await waitFor(() => {
      expect(
        screen.getByText("Daily Bank-GL 2026-05-01")
      ).toBeInTheDocument();
    });

    const input = screen.getByRole("textbox", { name: /search runs by name/i });
    await user.type(input, "Cross-System");

    // The Cross-System run should remain
    await waitFor(() => {
      expect(
        screen.getByText("Cross-System Recon 2026-05-10")
      ).toBeInTheDocument();
    });

    // A non-matching run should disappear
    expect(
      screen.queryByText("Daily Bank-GL 2026-05-01")
    ).not.toBeInTheDocument();
  });

  it("non-matching search shows empty state", async () => {
    const user = userEvent.setup();
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(
        screen.getByText("Daily Bank-GL 2026-05-01")
      ).toBeInTheDocument();
    });

    const input = screen.getByRole("textbox", { name: /search runs by name/i });
    await user.type(input, "xyznotexist99999");

    await waitFor(() => {
      expect(screen.getByText("No runs found")).toBeInTheDocument();
    });
  });

  it("clicking a run row navigates to /runs/{id}", async () => {
    const user = userEvent.setup();
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });

    await waitFor(() => {
      expect(
        screen.getByText("Daily Bank-GL 2026-05-01")
      ).toBeInTheDocument();
    });

    await user.click(screen.getByText("Daily Bank-GL 2026-05-01"));

    expect(mockPush).toHaveBeenCalledWith("/runs/run-acme-001");
  });

  it("shows loading skeletons before data resolves", () => {
    renderWithProviders(<RunsPage />, { tenantId: "tenant-acme" });
    // The 0-latency mock resolves on a microtask, so the synchronous render is loading
    expect(document.querySelector("[data-slot='skeleton']")).toBeTruthy();
  });
});
