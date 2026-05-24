import { type Page, expect } from "@playwright/test";

const RESEED_URL = "http://localhost:8080/api/dev/reseed";
const MAILPIT_API = "http://localhost:8025/api/v1";

// ---------------------------------------------------------------------------
// Backend helpers
// ---------------------------------------------------------------------------

/** Reset the DB to seeded state. Call in beforeEach for tests that mutate state. */
export async function reseed(): Promise<void> {
  const res = await fetch(RESEED_URL, { method: "POST" });
  if (!res.ok) throw new Error(`reseed failed: ${res.status}`);
}

// ---------------------------------------------------------------------------
// Auth UI helpers
// ---------------------------------------------------------------------------

/** Log in via the /login form and wait for /dashboard. */
export async function loginViaUI(
  page: Page,
  email: string,
  password: string
): Promise<void> {
  // Clear any stale session state from a previous test so we always land on the
  // login form rather than being auto-redirected or stuck in a loading state.
  await page.context().clearCookies();
  await page.goto("/login");

  // Wait for the login form to be interactive (auth "loading" resolves to "unauthenticated")
  await expect(page.getByRole("button", { name: /sign in/i })).toBeVisible({ timeout: 10_000 });

  await page.getByLabel("Email").fill(email);
  await page.getByLabel("Password").fill(password);
  await page.getByRole("button", { name: /sign in/i }).click();
  await expect(page).toHaveURL(/\/dashboard/, { timeout: 15_000 });
}

/** Log out via the user menu and wait for /login. */
export async function logoutViaUI(page: Page): Promise<void> {
  // Open user menu (trigger aria-label is "User menu — viewing as <name>")
  const menuTrigger = page.getByRole("button", { name: /user menu/i });
  await menuTrigger.click();

  // Wait for the dropdown menu to be open (role="menu")
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible({ timeout: 5_000 });

  // Click the Log out item
  const logoutItem = menu.getByRole("menuitem", { name: /log out/i });
  await expect(logoutItem).toBeVisible({ timeout: 3_000 });
  await logoutItem.click();

  await expect(page).toHaveURL(/\/login/, { timeout: 15_000 });
}

// ---------------------------------------------------------------------------
// Mailpit helpers
// ---------------------------------------------------------------------------

/** Delete all messages from Mailpit. */
export async function clearMailpit(): Promise<void> {
  await fetch(`${MAILPIT_API}/messages`, { method: "DELETE" });
}

interface MailpitMessage {
  ID: string;
  Subject: string;
  Snippet: string;
}

interface MailpitListResponse {
  messages: MailpitMessage[];
}

/**
 * Poll Mailpit for the latest message and extract the reset link.
 * Returns the full URL `http://localhost:3100/reset?token=...`.
 */
export async function latestResetLink(
  timeoutMs = 15_000
): Promise<string> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    const listRes = await fetch(`${MAILPIT_API}/messages`);
    const list: MailpitListResponse = await listRes.json();

    if (list.messages && list.messages.length > 0) {
      // Newest first (Mailpit returns them in descending Created order)
      const newest = list.messages[0];

      // Fetch the full message to get the Text body
      const msgRes = await fetch(`${MAILPIT_API}/message/${newest.ID}`);
      const msg: { Text?: string } = await msgRes.json();
      const body = msg.Text ?? "";

      const match = body.match(/http:\/\/localhost:3100\/reset\?token=[A-Fa-f0-9]+/);
      if (match) {
        return match[0];
      }
    }

    await new Promise((r) => setTimeout(r, 500));
  }

  throw new Error("Timed out waiting for reset email in Mailpit");
}
