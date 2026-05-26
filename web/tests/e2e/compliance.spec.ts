import { test, expect } from "@playwright/test";
import { loginViaUI, reseed } from "./helpers";

test.beforeEach(async () => {
  await reseed();
});

test("admin verifies the audit chain and anchors", async ({ page }) => {
  // Logging in as admin Ada emits an `auth.login.success` audit row.
  await loginViaUI(page, "ada@acme.test", "Password123!");

  await page.goto("/audit");

  // The login event is present in the audit table.
  await expect(
    page.getByText(/auth\.login\.success/).first()
  ).toBeVisible({ timeout: 10_000 });

  // ---------------------------------------------------------------------
  // Verify chain -> opens dialog -> Run -> expect "Valid".
  // ---------------------------------------------------------------------
  await page.getByRole("button", { name: /verify chain/i }).click();

  // Dialog title confirms we opened the right dialog.
  await expect(
    page.getByRole("heading", { name: /verify audit chain/i })
  ).toBeVisible();

  // Submit the verify dialog. The button text is "Run" (becomes "Verifying…").
  await page.getByRole("button", { name: /^run$/i }).click();

  // Result panel shows "Valid" inside the dialog (role="status").
  await expect(
    page.getByRole("status").getByText(/^valid$/i)
  ).toBeVisible({ timeout: 10_000 });

  // Close the dialog so the underlying "Anchor now" button is clickable.
  await page.getByRole("button", { name: /^close$/i }).click();

  // ---------------------------------------------------------------------
  // Anchor now -> sonner toast "Anchored at seq <N>".
  // ---------------------------------------------------------------------
  await page.getByRole("button", { name: /anchor now/i }).click();

  await expect(
    page.getByText(/anchored at seq\s*\d+/i)
  ).toBeVisible({ timeout: 10_000 });

  // ---------------------------------------------------------------------
  // Controls page -> click ISO27001:A.9.2.1 row -> navigate to /audit
  // filtered by its event kinds (admin.user.created is the first kind).
  // ---------------------------------------------------------------------
  await page.goto("/controls");
  await page.getByText(/A\.9\.2\.1/).first().click();
  await expect(page).toHaveURL(/\/audit\?.*kind=admin\.user\.created/);
});
