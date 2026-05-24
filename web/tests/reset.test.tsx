import { describe, it, expect, vi } from "vitest";
import { screen, waitFor, renderWithProviders, userEvent } from "./test-utils";

vi.mock("next/navigation", () => ({
  useRouter: vi.fn(() => ({ replace: vi.fn() })),
  useSearchParams: vi.fn(() => new URLSearchParams("token=test-token-123")),
}));

vi.mock("sonner", () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

vi.mock("@/lib/auth/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/auth/api")>();
  return {
    ...actual,
    resetRequest: vi.fn().mockResolvedValue(undefined),
  };
});

async function renderResetPage() {
  const { default: ResetPage } = await import("@/app/reset/page");
  return renderWithProviders(<ResetPage />);
}

describe("Reset password page", () => {
  it("renders the reset form when a token is present", async () => {
    await renderResetPage();

    await waitFor(() => {
      expect(screen.getByRole("heading", { name: /reset password/i })).toBeInTheDocument();
    });

    expect(screen.getByLabelText(/new password/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/confirm password/i)).toBeInTheDocument();
  });

  it("shows validation error when password is too short", async () => {
    const user = userEvent.setup();
    await renderResetPage();

    await waitFor(() => {
      expect(screen.getByLabelText(/new password/i)).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText(/new password/i), "short");
    await user.type(screen.getByLabelText(/confirm password/i), "short");
    await user.click(screen.getByRole("button", { name: /reset password/i }));

    await waitFor(() => {
      expect(screen.getByText(/at least 8 characters/i)).toBeInTheDocument();
    });
  });

  it("submits with valid data", async () => {
    const user = userEvent.setup();
    const { resetRequest } = await import("@/lib/auth/api");
    await renderResetPage();

    await waitFor(() => {
      expect(screen.getByLabelText(/new password/i)).toBeInTheDocument();
    });

    await user.type(screen.getByLabelText(/new password/i), "newpassword123");
    await user.type(screen.getByLabelText(/confirm password/i), "newpassword123");
    await user.click(screen.getByRole("button", { name: /reset password/i }));

    await waitFor(() => {
      expect(resetRequest).toHaveBeenCalledWith("test-token-123", "newpassword123");
    });
  });
});
