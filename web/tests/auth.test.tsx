import { describe, it, expect } from "vitest";
import { renderWithProviders, screen, waitFor } from "./test-utils";
import { useAuth } from "@/lib/auth/auth-provider";
import type { Membership } from "@/lib/domain/types";

// -----------------------------------------------------------------------
// Helper component to expose auth context values in tests
// -----------------------------------------------------------------------
function AuthInspector() {
  const { user, activeTenant, memberships } = useAuth();
  return (
    <div>
      <span data-testid="user-id">{user?.id ?? "none"}</span>
      <span data-testid="user-role">{user?.role ?? "none"}</span>
      <span data-testid="tenant-id">{activeTenant?.id ?? "none"}</span>
      <span data-testid="tenant-name">{activeTenant?.name ?? "none"}</span>
      <span data-testid="memberships-count">{memberships.length}</span>
    </div>
  );
}

// -----------------------------------------------------------------------
// MockAuthProvider — switchTenant and changePassword stubs
// -----------------------------------------------------------------------
describe("MockAuthProvider provides correct session values", () => {
  it("exposes the seeded user and tenant", async () => {
    renderWithProviders(<AuthInspector />, {
      currentUserId: "user-mia",
      tenantId: "tenant-acme",
    });

    await waitFor(() => {
      expect(screen.getByTestId("user-id")).toHaveTextContent("user-mia");
      expect(screen.getByTestId("tenant-id")).toHaveTextContent("tenant-acme");
      expect(screen.getByTestId("tenant-name")).toHaveTextContent("Acme Capital");
    });
  });

  it("exposes memberships when seeded", async () => {
    const memberships: Membership[] = [
      { tenantId: "tenant-acme", tenantName: "Acme Capital", role: "admin" },
      { tenantId: "tenant-globex", tenantName: "Globex Markets", role: "operator" },
    ];

    renderWithProviders(<AuthInspector />, {
      currentUserId: "user-ada",
      memberships,
    });

    await waitFor(() => {
      expect(screen.getByTestId("memberships-count")).toHaveTextContent("2");
    });
  });
});
