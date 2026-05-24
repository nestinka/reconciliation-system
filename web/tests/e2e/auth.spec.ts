import { test, expect } from "@playwright/test";
import {
  reseed,
  loginViaUI,
  logoutViaUI,
  clearMailpit,
  latestResetLink,
} from "./helpers";

const MIA_EMAIL = "mia@acme.test";
const ADA_EMAIL = "ada@acme.test";
const PASSWORD = "Password123!";

test.beforeEach(async () => {
  await reseed();
});

test.describe("Auth – redirect and session", () => {
  test("redirects to /login when unauthenticated", async ({ page }) => {
    await page.goto("/dashboard");
    await expect(page).toHaveURL(/\/login/, { timeout: 10_000 });
  });

  test("login then reload stays authenticated", async ({ page }) => {
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
    await page.reload();
    // Should remain on an authed page (not bounced back to /login)
    await expect(page).not.toHaveURL(/\/login/, { timeout: 10_000 });
    await expect(
      page.getByRole("heading", { name: "Dashboard" })
    ).toBeVisible();
  });

  test("logout returns to login and /dashboard bounces back", async ({
    page,
  }) => {
    await loginViaUI(page, MIA_EMAIL, PASSWORD);
    await logoutViaUI(page);

    // Should be on /login now
    await expect(page).toHaveURL(/\/login/);

    // Attempting to visit /dashboard should redirect back to /login
    await page.goto("/dashboard");
    await expect(page).toHaveURL(/\/login/, { timeout: 10_000 });
  });
});

test.describe("Auth – tenant switcher", () => {
  test("tenant switch re-scopes (ada has Acme and Globex)", async ({
    page,
  }) => {
    await loginViaUI(page, ADA_EMAIL, PASSWORD);

    // The tenant switcher trigger should be visible
    const switcher = page.getByRole("button", { name: /switch tenant/i });
    await expect(switcher).toBeVisible();

    // Open the dropdown
    await switcher.click();

    // Both tenants should be listed
    await expect(page.getByRole("menuitemradio", { name: /acme/i })).toBeVisible();
    await expect(page.getByRole("menuitemradio", { name: /globex/i })).toBeVisible();

    // Find whichever tenant is NOT currently active and switch to it
    const acmeItem = page.getByRole("menuitemradio", { name: /acme/i });
    const globexItem = page.getByRole("menuitemradio", { name: /globex/i });

    // Determine the inactive one by checked state
    const acmeChecked = await acmeItem.getAttribute("aria-checked");
    const inactiveItem = acmeChecked === "true" ? globexItem : acmeItem;
    const expectedTenantName = acmeChecked === "true" ? /globex/i : /acme/i;

    await inactiveItem.click();

    // The trigger text should update to the new tenant name
    await expect(switcher).toContainText(expectedTenantName, { timeout: 5_000 });

    // Page is still functional
    await expect(
      page.getByRole("heading", { name: "Dashboard" })
    ).toBeVisible();
  });
});

test.describe("Auth – RBAC: admin", () => {
  test("admin can open Users and create a user", async ({ page }) => {
    await loginViaUI(page, ADA_EMAIL, PASSWORD);

    // Users nav item is visible for admins
    await expect(
      page.getByRole("link", { name: "Users" })
    ).toBeVisible();

    // Go to the Users page
    await page.goto("/users");
    await expect(
      page.getByRole("heading", { name: "Users" })
    ).toBeVisible();

    // Open the Add user dialog
    await page.getByRole("button", { name: /add user/i }).click();
    const dialog = page.getByRole("dialog", { name: /add user/i });
    await expect(dialog).toBeVisible();

    // Fill in the form
    const timestamp = Date.now();
    const newEmail = `e2e+${timestamp}@acme.test`;
    const newName = `E2E User ${timestamp}`;

    await dialog.getByLabel("Name").fill(newName);
    await dialog.getByLabel("Email").fill(newEmail);

    // Role select — default is already "operator", but set it explicitly
    // Scope to the dialog to avoid strict mode violation with table role selects
    const roleSelect = dialog.getByLabel("Role");
    await roleSelect.click();
    await page.getByRole("option", { name: "Operator" }).click();

    await dialog.getByLabel("Temporary password").fill(PASSWORD);

    // Submit
    await dialog.getByRole("button", { name: /create user/i }).click();

    // Dialog closes after success
    await expect(
      page.getByRole("dialog", { name: /add user/i })
    ).not.toBeVisible({ timeout: 10_000 });

    // New user appears in the table
    await expect(page.getByText(newEmail)).toBeVisible({ timeout: 10_000 });
  });
});

test.describe("Auth – RBAC: non-admin", () => {
  test("non-admin has no Users access", async ({ page }) => {
    await loginViaUI(page, MIA_EMAIL, PASSWORD);

    // Users nav item must NOT be visible for a regular operator
    await expect(
      page.getByRole("link", { name: "Users" })
    ).not.toBeVisible();

    // Navigating directly to /users should redirect away
    await page.goto("/users");
    await expect(page).not.toHaveURL(/\/users/, { timeout: 10_000 });
    // Should land back on /dashboard
    await expect(page).toHaveURL(/\/dashboard/, { timeout: 10_000 });
  });
});

test.describe("Auth – password reset", () => {
  test("password reset via email works end-to-end", async ({ page }) => {
    await clearMailpit();

    // Go to /forgot, submit Mia's email
    await page.goto("/forgot");
    await page.getByLabel("Email").fill(MIA_EMAIL);
    await page.getByRole("button", { name: /send reset link/i }).click();

    // Confirmation copy should appear
    await expect(
      page.getByText(/we['']ve sent a reset link/i)
    ).toBeVisible({ timeout: 10_000 });

    // Fetch the reset link from Mailpit
    const resetLink = await latestResetLink();

    // Navigate to the reset page
    await page.goto(resetLink);

    // Fill in new password
    const newPassword = "NewPassword123!";
    await page.getByLabel("New password").fill(newPassword);
    await page.getByLabel("Confirm password").fill(newPassword);
    await page.getByRole("button", { name: /reset password/i }).click();

    // Should be redirected to /login
    await expect(page).toHaveURL(/\/login/, { timeout: 10_000 });

    // Login with the NEW password succeeds (must happen before the next reseed)
    await loginViaUI(page, MIA_EMAIL, newPassword);
    await expect(
      page.getByRole("heading", { name: "Dashboard" })
    ).toBeVisible();
  });
});
