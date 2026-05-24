import { describe, it, expect } from "vitest";
import { screen, waitFor, renderWithProviders } from "./test-utils";
import { UserMenu } from "@/components/app/user-menu";

/**
 * The base-ui Menu doesn't open in jsdom (floating-ui needs real browser geometry),
 * so we test the trigger state and underlying context interactions.
 */
describe("UserMenu", () => {
  it("shows the current user (Mia) after data loads", async () => {
    renderWithProviders(<UserMenu />);

    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /user menu/i });
      expect(trigger).toHaveTextContent("Mia");
    });
  });

  it("trigger button has an accessible aria-label mentioning the user", async () => {
    renderWithProviders(<UserMenu />);

    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /viewing as Mia/i })
      ).toBeInTheDocument();
    });
  });

  it("shows initials derived from the user name", async () => {
    renderWithProviders(<UserMenu />);

    // Mia (single name) → "M" initials in the avatar
    await waitFor(() => {
      expect(screen.getByText("M")).toBeInTheDocument();
    });
  });

  it("renders the correct user when currentUserId option is provided", async () => {
    /**
     * User switching via setCurrentUserId is no longer supported (auth is
     * token-based; the active user comes from the JWT/session). This test
     * verifies the UserMenu renders the correct user when the session is
     * seeded with a specific user via renderWithProviders options.
     */
    renderWithProviders(<UserMenu />, { currentUserId: "user-theo" });

    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /viewing as Theo/i });
      expect(trigger).toHaveTextContent("Theo");
    });
  });
});
