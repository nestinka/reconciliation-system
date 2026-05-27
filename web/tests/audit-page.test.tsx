import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen, waitFor, renderWithProviders, userEvent } from "./test-utils";

// next/navigation must be mocked because the page calls useRouter().
vi.mock("next/navigation", () => ({
  useRouter: vi.fn(() => ({ replace: vi.fn(), push: vi.fn() })),
}));

// sonner toast — mock so we can assert without depending on its UI.
vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

// The page itself uses dynamic-ish imports under the (app) route; we load it
// lazily so the mocks above are in place before the module evaluates.
async function renderAuditPage(
  options: Parameters<typeof renderWithProviders>[1] = {}
) {
  const { default: AuditPage } = await import("@/app/(app)/audit/page");
  return renderWithProviders(<AuditPage />, options);
}

describe("Audit Log page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders rows from useAudit", async () => {
    await renderAuditPage({ currentUserId: "user-ada" });

    // The MockApiClient seeds three deterministic audit events for tenant-acme.
    // The middle one is "data.ingest.completed".
    await waitFor(() => {
      expect(screen.getByText("data.ingest.completed")).toBeInTheDocument();
    });
    // The other two seeded kinds should appear too.
    expect(screen.getByText("auth.login.success")).toBeInTheDocument();
    expect(screen.getByText("case.assigned")).toBeInTheDocument();
  });

  it("verify chain button shows a result panel", async () => {
    const user = userEvent.setup();
    await renderAuditPage({ currentUserId: "user-ada" });

    // Wait for the page to mount (rows loaded).
    await waitFor(() => {
      expect(screen.getByText("data.ingest.completed")).toBeInTheDocument();
    });

    // Open the verify dialog
    await user.click(screen.getByRole("button", { name: /verify chain/i }));

    // The dialog should appear with a "Run" button. Click it to run the verify.
    const runBtn = await screen.findByRole("button", { name: /^run$/i });
    await user.click(runBtn);

    // The MockApiClient returns { status: "valid", checked: 3 } for tenant-acme.
    await waitFor(() => {
      expect(screen.getByText(/valid/i)).toBeInTheDocument();
    });
    // The "checked" counter should be displayed.
    expect(screen.getByText(/checked:/i)).toBeInTheDocument();
  });

  it("anchor button calls anchorAudit and reports the seq via toast", async () => {
    const user = userEvent.setup();
    const { toast } = await import("sonner");

    await renderAuditPage({ currentUserId: "user-ada" });

    // Wait until rows have rendered (and thus the toolbar is mounted).
    await waitFor(() => {
      expect(screen.getByText("data.ingest.completed")).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /anchor now/i }));

    // MockApiClient.anchorAudit returns { anchorSeq: 1, hash: "0…" }.
    await waitFor(() => {
      expect(toast.success).toHaveBeenCalledWith(
        expect.stringMatching(/anchored at seq 1/i)
      );
    });
  });

  it("redirects non-admin users to /dashboard", async () => {
    const mockReplace = vi.fn();
    const { useRouter } = await import("next/navigation");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    vi.mocked(useRouter).mockReturnValue({
      replace: mockReplace,
      push: vi.fn(),
    } as any);

    await renderAuditPage({ currentUserId: "user-mia" }); // operator

    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith("/dashboard");
    });
  });
});
