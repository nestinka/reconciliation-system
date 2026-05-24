import { describe, it, expect, vi } from "vitest";
import { screen, waitFor, renderWithProviders } from "./test-utils";

vi.mock("next/navigation", () => ({
  useRouter: vi.fn(() => ({ replace: vi.fn() })),
}));

// Lazy-import the page to avoid issues with next/navigation mock
async function renderUsersPage(options: Parameters<typeof renderWithProviders>[1] = {}) {
  const { default: UsersPage } = await import("@/app/(app)/users/page");
  return renderWithProviders(<UsersPage />, options);
}

describe("Admin Users page", () => {
  it("renders the users table for admin users", async () => {
    const { container } = await renderUsersPage({ currentUserId: "user-ada" });
    // The Users heading and Add user button should be present
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: /users/i })).toBeInTheDocument();
    });
    expect(container).toBeTruthy();
  });

  it("lists users from MockApiClient", async () => {
    await renderUsersPage({ currentUserId: "user-ada" });

    await waitFor(() => {
      // The fixture has Mia, Sam, Theo, Ada — all should be visible in the table
      expect(screen.getByText("Mia")).toBeInTheDocument();
      expect(screen.getByText("Sam")).toBeInTheDocument();
    });
  });

  it("shows Add user button for admin", async () => {
    await renderUsersPage({ currentUserId: "user-ada" });

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /add user/i })).toBeInTheDocument();
    });
  });

  it("redirects non-admin users away", async () => {
    const mockReplace = vi.fn();
    const { useRouter } = await import("next/navigation");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    vi.mocked(useRouter).mockReturnValue({ replace: mockReplace } as any);

    await renderUsersPage({ currentUserId: "user-mia" }); // operator

    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith("/dashboard");
    });
  });
});

describe("MockApiClient user CRUD", () => {
  it("createUser adds to the users list", async () => {
    const { MockApiClient } = await import("@/lib/api/mock");
    const client = new MockApiClient({ latencyMs: 0 });

    const before = await client.listUsers("tenant-acme");
    const beforeCount = before.length;

    const newUser = await client.createUser("tenant-acme", {
      name: "Test User",
      email: "test@example.com",
      role: "operator",
      password: "password123",
    });

    expect(newUser.name).toBe("Test User");
    expect(newUser.email).toBe("test@example.com");
    expect(newUser.role).toBe("operator");

    const after = await client.listUsers("tenant-acme");
    expect(after.length).toBe(beforeCount + 1);
    expect(after.some((u) => u.id === newUser.id)).toBe(true);
  });

  it("updateUser changes role in the list", async () => {
    const { MockApiClient } = await import("@/lib/api/mock");
    const client = new MockApiClient({ latencyMs: 0 });

    await client.updateUser("tenant-acme", "user-mia", { role: "approver" });

    const users = await client.listUsers("tenant-acme");
    const mia = users.find((u) => u.id === "user-mia");
    expect(mia?.role).toBe("approver");
  });

  it("updateUser disables a user", async () => {
    const { MockApiClient } = await import("@/lib/api/mock");
    const client = new MockApiClient({ latencyMs: 0 });

    await client.updateUser("tenant-acme", "user-mia", { disabled: true });

    const users = await client.listUsers("tenant-acme");
    const mia = users.find((u) => u.id === "user-mia");
    expect(mia?.disabled).toBe(true);
  });

  it("deleteUser removes from the list", async () => {
    const { MockApiClient } = await import("@/lib/api/mock");
    const client = new MockApiClient({ latencyMs: 0 });

    const before = await client.listUsers("tenant-acme");
    const hasMia = before.some((u) => u.id === "user-mia");
    expect(hasMia).toBe(true);

    await client.deleteUser("tenant-acme", "user-mia");

    const after = await client.listUsers("tenant-acme");
    expect(after.some((u) => u.id === "user-mia")).toBe(false);
  });
});
