import { describe, it, expect, vi, beforeEach } from "vitest";
import { screen, waitFor, renderWithProviders, userEvent } from "./test-utils";

// next/navigation must be mocked because the page calls useRouter().
vi.mock("next/navigation", () => ({
  useRouter: vi.fn(() => ({ replace: vi.fn(), push: vi.fn() })),
}));

// Lazy-load the page so the mocks above are in place before the module evaluates.
async function renderControlsPage(
  options: Parameters<typeof renderWithProviders>[1] = {}
) {
  const { default: ControlsPage } = await import("@/app/(app)/controls/page");
  return renderWithProviders(<ControlsPage />, options);
}

describe("Controls page", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders three framework sections from useControls", async () => {
    await renderControlsPage({ currentUserId: "user-ada" });

    // MockApiClient.listControls returns 3 entries: ISO 27001, SOC 2, FCA.
    await waitFor(() => {
      expect(screen.getByText("ISO 27001")).toBeInTheDocument();
    });
    expect(screen.getByText("SOC 2")).toBeInTheDocument();
    expect(screen.getByText("FCA")).toBeInTheDocument();
  });

  it("renders control ids in mono and shows their event-kind chips", async () => {
    await renderControlsPage({ currentUserId: "user-ada" });

    await waitFor(() => {
      expect(screen.getByText("ISO27001:A.9.4.2")).toBeInTheDocument();
    });
    expect(screen.getByText("SOC2:CC6.1")).toBeInTheDocument();
    expect(screen.getByText("FCA:SYSC9.1")).toBeInTheDocument();

    // The FCA control's eventKinds include data.ingest.completed and data.run.created.
    expect(screen.getByText("data.ingest.completed")).toBeInTheDocument();
    expect(screen.getByText("data.run.created")).toBeInTheDocument();
  });

  it("clicking a control row navigates to /audit with all of its kind params", async () => {
    const mockPush = vi.fn();
    const { useRouter } = await import("next/navigation");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    vi.mocked(useRouter).mockReturnValue({
      replace: vi.fn(),
      push: mockPush,
    } as any);

    const user = userEvent.setup();
    await renderControlsPage({ currentUserId: "user-ada" });

    const row = await screen.findByRole("button", {
      name: /open audit log filtered by ISO27001:A\.9\.4\.2/i,
    });
    await user.click(row);

    expect(mockPush).toHaveBeenCalledTimes(1);
    const href = mockPush.mock.calls[0][0] as string;
    expect(href).toMatch(/^\/audit\?/);
    expect(href).toContain("kind=auth.login.success");
    expect(href).toContain("kind=auth.login.failure");
    expect(href).toContain("kind=auth.lockout");
  });

  it("redirects non-admin users to /dashboard", async () => {
    const mockReplace = vi.fn();
    const { useRouter } = await import("next/navigation");
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    vi.mocked(useRouter).mockReturnValue({
      replace: mockReplace,
      push: vi.fn(),
    } as any);

    await renderControlsPage({ currentUserId: "user-mia" }); // operator

    await waitFor(() => {
      expect(mockReplace).toHaveBeenCalledWith("/dashboard");
    });
  });
});
