import { describe, it, expect } from "vitest";
import { screen, waitFor, renderWithProviders } from "./test-utils";
import { TenantSwitcher } from "@/components/app/tenant-switcher";

const TWO_MEMBERSHIPS = [
  { tenantId: "tenant-acme", tenantName: "Acme Capital", role: "admin" as const },
  { tenantId: "tenant-globex", tenantName: "Globex Markets", role: "operator" as const },
];

describe("TenantSwitcher", () => {
  it("shows the active tenant name (static label) with single/no memberships", async () => {
    renderWithProviders(<TenantSwitcher />);

    // With 0 memberships (default), renders a static label not a button
    await waitFor(() => {
      expect(screen.getByText("Acme Capital")).toBeInTheDocument();
    });
  });

  it("shows the active tenant name after data loads with multiple memberships", async () => {
    renderWithProviders(<TenantSwitcher />, { memberships: TWO_MEMBERSHIPS });

    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /switch tenant/i });
      expect(trigger).toHaveTextContent("Acme Capital");
    });
  });

  it("trigger button has accessible label when multiple memberships", async () => {
    renderWithProviders(<TenantSwitcher />, { memberships: TWO_MEMBERSHIPS });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /switch tenant/i })).toBeInTheDocument();
    });
  });

  it("renders the correct tenant when tenantId option is provided", async () => {
    /**
     * Tenant switching is auth-based; the active tenant comes from the session.
     * This test verifies TenantSwitcher renders the correct tenant when seeded
     * with a specific tenantId via renderWithProviders options.
     */
    renderWithProviders(<TenantSwitcher />, {
      tenantId: "tenant-globex",
      memberships: [
        { tenantId: "tenant-acme", tenantName: "Acme Capital", role: "admin" as const },
        { tenantId: "tenant-globex", tenantName: "Globex Markets", role: "operator" as const },
      ],
    });

    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /switch tenant/i });
      expect(trigger).toHaveTextContent("Globex Markets");
    });
  });
});
