import { describe, it, expect } from "vitest";
import { screen, waitFor, renderWithProviders } from "./test-utils";
import { TenantSwitcher } from "@/components/app/tenant-switcher";

/**
 * The base-ui Menu doesn't open in jsdom (floating-ui needs real browser geometry),
 * so we test:
 *  - the trigger renders and shows the active tenant after data loads
 *  - the trigger has an accessible label
 *
 * Interaction tests that require the dropdown to be open are covered by e2e tests.
 */
describe("TenantSwitcher", () => {
  it("shows the active tenant name after data loads", async () => {
    renderWithProviders(<TenantSwitcher />);

    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /switch tenant/i });
      expect(trigger).toHaveTextContent("Acme Capital");
    });
  });

  it("trigger button has accessible label", async () => {
    renderWithProviders(<TenantSwitcher />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /switch tenant/i })).toBeInTheDocument();
    });
  });

  it("is disabled while loading", () => {
    renderWithProviders(<TenantSwitcher />);
    // On first synchronous render (before the 0-latency query resolves) the
    // trigger is disabled so the menu can't be opened with no data.
    expect(
      screen.getByRole("button", { name: /switch tenant/i })
    ).toBeDisabled();
  });

  it("becomes enabled once tenants load", async () => {
    renderWithProviders(<TenantSwitcher />);
    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /switch tenant/i });
      expect(trigger).not.toBeDisabled();
    });
  });

  it("renders the correct tenant when tenantId option is provided", async () => {
    /**
     * Tenant switching via setTenantId is deferred (auth is token-based; the
     * active tenant comes from the session). This test verifies the
     * TenantSwitcher renders the correct tenant when the session is seeded
     * with a specific tenantId via renderWithProviders options.
     */
    renderWithProviders(<TenantSwitcher />, { tenantId: "tenant-globex" });

    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /switch tenant/i });
      expect(trigger).toHaveTextContent("Globex Markets");
    });
  });
});
