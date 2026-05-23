import { describe, it, expect } from "vitest";
import { screen, waitFor, userEvent, renderWithProviders } from "./test-utils";
import { UserMenu } from "@/components/app/user-menu";
import { useCurrentUserId } from "@/lib/providers/current-user-provider";

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

  it("setCurrentUserId changes the user shown in the trigger", async () => {
    /**
     * We cannot open the menu in jsdom, but we can test the underlying context
     * by rendering a helper that calls setCurrentUserId directly.
     */
    function ContextChanger() {
      const { setCurrentUserId } = useCurrentUserId();
      return (
        <button onClick={() => setCurrentUserId("user-sam")}>
          Switch to Sam
        </button>
      );
    }

    renderWithProviders(
      <>
        <UserMenu />
        <ContextChanger />
      </>
    );

    // Wait for Mia to load
    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /viewing as Mia/i });
      expect(trigger).toHaveTextContent("Mia");
    });

    // Switch user via the test helper
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: /switch to sam/i }));

    // Now the trigger should show Sam
    await waitFor(() => {
      const trigger = screen.getByRole("button", { name: /viewing as Sam/i });
      expect(trigger).toHaveTextContent("Sam");
    });
  });
});
