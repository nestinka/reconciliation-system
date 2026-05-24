import { test, expect } from "@playwright/test";
import { reseed, loginViaUI, logoutViaUI } from "./helpers";

/**
 * Operator loop E2E test
 *
 * Covers the core operator workflow and the four-eyes approval gate:
 *   1. Root → redirects to Dashboard
 *   2. Navigate to Exceptions via sidebar
 *   3. Open the pending-approval case (case-pending)
 *   4. As Mia (operator + maker): Approve button is DISABLED with four-eyes message
 *   5. Logout and login as Theo (approver, not the maker)
 *   6. Approve button is ENABLED; click it
 *   7. Case resolves: status pill shows "Resolved", ApprovalBar disappears
 */

const MIA_EMAIL = "mia@acme.test";
const THEO_EMAIL = "theo@acme.test";
const PASSWORD = "Password123!";

// Reset the backend to seeded state before each test (the four-eyes flow mutates case-pending).
test.beforeEach(async () => {
  await reseed();
});

test.describe("Operator loop – four-eyes approval flow", () => {
  test("1. Root redirects to dashboard and shows heading + KPI text", async ({
    page,
  }) => {
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
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
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
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
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
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
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
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

  test("5–7. Login as Theo, Approve enabled, click resolves the case", async ({
    page,
  }) => {
    // Login as Mia first to confirm the button is disabled
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
    await page.goto("/cases/case-pending");

    // Wait for the case detail to fully render
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).toBeVisible();
    // Confirm disabled as Mia
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeDisabled();

    // Step 5: Logout and login as Theo
    await logoutViaUI(page);
    await loginViaUI(page, THEO_EMAIL, PASSWORD);
    await page.goto("/cases/case-pending");

    // Step 6: Approve button is now ENABLED
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeEnabled();

    // Click Approve
    await page.getByRole("button", { name: /approve/i }).click();

    // Step 7: Case resolves
    // The "Pending four-eyes approval" section should be unmounted entirely
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).not.toBeAttached();

    // Status pill shows "Resolved" (both the header pill and the break-context
    // status now read "Resolved" because the backend transitions the linked
    // break too — scope to the first match to avoid strict-mode ambiguity).
    await expect(page.getByText("Resolved").first()).toBeVisible();
  });

  test("Full operator loop: dashboard → exceptions → pending case → switch user → approve", async ({
    page,
  }) => {
    await loginViaUI(page, MIA_EMAIL, PASSWORD);

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

    // 5. Logout and login as Theo
    await logoutViaUI(page);
    await loginViaUI(page, THEO_EMAIL, PASSWORD);
    await page.goto("/cases/case-pending");

    // 6. Approve is now enabled
    await expect(
      page.getByRole("button", { name: /approve/i })
    ).toBeEnabled();
    await page.getByRole("button", { name: /approve/i }).click();

    // 7. Case resolves — approval bar unmounted, resolved status visible
    await expect(
      page.getByRole("region", { name: /four-eyes approval/i })
    ).not.toBeAttached();
    await expect(page.getByText("Resolved").first()).toBeVisible();
  });
});
