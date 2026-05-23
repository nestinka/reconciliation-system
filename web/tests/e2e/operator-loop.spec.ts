import { test, expect, type Page } from "@playwright/test";

/**
 * Operator loop E2E test
 *
 * Covers the core operator workflow and the four-eyes approval gate:
 *   1. Root → redirects to Dashboard
 *   2. Navigate to Exceptions via sidebar
 *   3. Open the pending-approval case (case-pending)
 *   4. As Mia (default maker): Approve button is DISABLED with four-eyes message
 *   5. Switch to Theo (approver) via UserMenu
 *   6. Approve button is ENABLED; click it
 *   7. Case resolves: status pill shows "Resolved", ApprovalBar disappears
 */

// Seed tenant + current user via localStorage before page load so we don't
// depend on any previous localStorage state in the browser.
async function seedStorage(page: Page, userId = "user-mia") {
  await page.addInitScript(
    ({ tenantId, currentUserId }) => {
      window.localStorage.setItem("recon:activeTenantId", tenantId);
      window.localStorage.setItem("recon:currentUserId", currentUserId);
    },
    { tenantId: "tenant-acme", currentUserId: userId }
  );
}

/**
 * Switch the current user via the UserMenu in the top bar.
 * The @base-ui/react RadioItem does not always auto-close the menu in Playwright.
 * We explicitly dismiss the menu with Escape after selecting so subsequent
 * pointer interactions are not blocked by the open dropdown.
 */
async function switchUserViaMenu(page: Page, userName: string) {
  // Open the user menu (trigger aria-label contains "viewing as")
  await page.getByRole("button", { name: /viewing as/i }).click();

  // Wait for the menu to be visible
  await expect(
    page.getByRole("menuitemradio", { name: new RegExp(userName, "i") })
  ).toBeVisible();

  // Click the target user radio item
  await page.getByRole("menuitemradio", { name: new RegExp(userName, "i") }).click();

  // @base-ui/react may keep the menu open after a radio click in the test
  // environment. Close it explicitly with Escape and wait for it to disappear.
  await page.keyboard.press("Escape");

  // Wait for the menu to close — the menu element should no longer be expanded
  await expect(
    page.getByRole("button", { name: /viewing as/i })
  ).not.toHaveAttribute("aria-expanded", "true");
}

test.describe("Operator loop – four-eyes approval flow", () => {
  test("1. Root redirects to dashboard and shows heading + KPI text", async ({
    page,
  }) => {
    await seedStorage(page);
    await page.goto("/");

    // Should land on /dashboard (redirect from /)
    await expect(page).toHaveURL(/\/dashboard/);

    // Dashboard heading
    await expect(
      page.getByRole("heading", { name: "Dashboard" })
    ).toBeVisible();

    // KPI label — use exact match to avoid "sum across open breaks"
    await expect(
      page.getByText("Open breaks", { exact: true })
    ).toBeVisible();
  });

  test("2. Navigate to Exceptions via sidebar – breaks table renders", async ({
    page,
  }) => {
    await seedStorage(page);
    await page.goto("/dashboard");

    // Click the "Exceptions" nav link in the sidebar
    await page.getByRole("link", { name: "Exceptions" }).click();

    await expect(page).toHaveURL(/\/exceptions/);

    // Page header — use heading role to avoid strict mode violation with nav link
    await expect(
      page.getByRole("heading", { name: "Exceptions" })
    ).toBeVisible();

    // At least one row should appear (status pill "Open" or "Investigating")
    await expect(page.getByText("Open").first()).toBeVisible();
  });

  test("3. Open the pending-approval case directly – case detail renders", async ({
    page,
  }) => {
    await seedStorage(page);
    await page.goto("/cases/case-pending");

    // Case detail heading includes the break id
    await expect(
      page.getByRole("heading", { name: /investigate break-pending/i })
    ).toBeVisible();

    // The four-eyes approval section must be present
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).toBeVisible();
  });

  test("4. As Mia (maker): Approve button is disabled with four-eyes message", async ({
    page,
  }) => {
    // Default user is user-mia (the maker of the pending proposal)
    await seedStorage(page, "user-mia");
    await page.goto("/cases/case-pending");

    // Wait for the approval bar to appear
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).toBeVisible();

    // Approve button must be disabled
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeDisabled();

    // Four-eyes reason text must be visible (the note paragraph)
    await expect(
      page.getByText(/a different approver must review/i)
    ).toBeVisible();
  });

  test("5–7. Switch to Theo, Approve enabled, click resolves the case", async ({
    page,
  }) => {
    // Start as Mia, then switch to Theo via the UserMenu
    await seedStorage(page, "user-mia");
    await page.goto("/cases/case-pending");

    // Wait for the case detail to fully render
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).toBeVisible();
    // Confirm disabled as Mia
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeDisabled();

    // Step 5: Switch user to Theo via UserMenu
    await switchUserViaMenu(page, "Theo");

    // Step 6: Approve button is now ENABLED
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeEnabled();

    // Click Approve
    await page.getByRole("button", { name: /approve/i }).click();

    // Step 7: Case resolves
    // The "Pending four-eyes approval" section should disappear
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).not.toBeVisible();

    // Status pill shows "Resolved"
    await expect(page.getByText("Resolved")).toBeVisible();
  });

  test("Full operator loop: dashboard → exceptions → pending case → switch user → approve", async ({
    page,
  }) => {
    await seedStorage(page, "user-mia");

    // 1. Start at root
    await page.goto("/");
    await expect(page).toHaveURL(/\/dashboard/);
    await expect(
      page.getByRole("heading", { name: "Dashboard" })
    ).toBeVisible();

    // 2. Navigate to Exceptions
    await page.getByRole("link", { name: "Exceptions" }).click();
    await expect(page).toHaveURL(/\/exceptions/);
    await expect(
      page.getByRole("heading", { name: "Exceptions" })
    ).toBeVisible();
    await expect(page.getByText("Open").first()).toBeVisible();

    // 3. Navigate to the pending case directly to ensure we open the right one
    await page.goto("/cases/case-pending");
    await expect(
      page.getByRole("heading", { name: /investigate break-pending/i })
    ).toBeVisible();

    // 4. Verify Approve is disabled as Mia
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeDisabled();

    // 5. Switch to Theo via UserMenu
    await switchUserViaMenu(page, "Theo");

    // 6. Approve is now enabled
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeEnabled();
    await page.getByRole("button", { name: /approve/i }).click();

    // 7. Case resolves — approval bar gone, resolved status visible
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).not.toBeVisible();
    await expect(page.getByText("Resolved")).toBeVisible();
  });
});
