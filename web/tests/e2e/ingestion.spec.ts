import { test, expect } from "@playwright/test";
import path from "node:path";
import { reseed, loginViaUI } from "./helpers";

const ADA_EMAIL = "ada@acme.test";
const PASSWORD = "Password123!";

const BANK_CSV = path.join(__dirname, "fixtures/bank.csv");
const LEDGER_XML = path.join(__dirname, "fixtures/ledger.camt053.xml");

test.beforeEach(async () => {
  await reseed();
});

test("operator ingests two files and creates a run", async ({ page }) => {
  await loginViaUI(page, ADA_EMAIL, PASSWORD);

  // ── Create a bank source ─────────────────────────────────────────────────
  await page.goto("/sources");
  await page.getByRole("button", { name: /new source/i }).click();

  const newSourceDialog = page.getByRole("dialog");
  await expect(newSourceDialog).toBeVisible();

  await newSourceDialog.getByLabel("Name").fill("E2E Bank");
  // Kind defaults to "bank" — no change needed
  await newSourceDialog.getByLabel("Currency").fill("GBP");
  await newSourceDialog.getByRole("button", { name: /create source/i }).click();

  // Dialog closes; source row appears in the table
  await expect(newSourceDialog).not.toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("cell", { name: "E2E Bank" })).toBeVisible();

  // ── Upload CSV into the bank source ──────────────────────────────────────
  await page
    .getByRole("row", { name: /E2E Bank/ })
    .getByRole("button", { name: /upload/i })
    .click();

  const uploadDialog = page.getByRole("dialog");
  await expect(uploadDialog).toBeVisible();

  // Set description column to 3 (CSV layout: ref=0, date=1, amount=2, desc=3)
  await uploadDialog.getByLabel("Description col").fill("3");

  // Attach the file to the hidden file input
  await uploadDialog.getByLabel("File").setInputFiles(BANK_CSV);

  await uploadDialog.getByRole("button", { name: /^upload$/i }).click();

  // Success toast: "2 transactions ingested."
  await expect(page.getByText(/2 transactions ingested/i)).toBeVisible({
    timeout: 15_000,
  });

  // Dialog closes after success
  await expect(uploadDialog).not.toBeVisible({ timeout: 10_000 });

  // ── Create a ledger source ───────────────────────────────────────────────
  await page.getByRole("button", { name: /new source/i }).click();

  const newSourceDialog2 = page.getByRole("dialog");
  await expect(newSourceDialog2).toBeVisible();

  await newSourceDialog2.getByLabel("Name").fill("E2E Ledger");
  // Change kind to "ledger"
  await newSourceDialog2.getByLabel("Kind").click();
  await page.getByRole("option", { name: "Ledger" }).click();
  await newSourceDialog2.getByLabel("Currency").fill("GBP");
  await newSourceDialog2.getByRole("button", { name: /create source/i }).click();

  await expect(newSourceDialog2).not.toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("cell", { name: "E2E Ledger" })).toBeVisible();

  // ── Upload CAMT.053 into the ledger source ────────────────────────────────
  await page
    .getByRole("row", { name: /E2E Ledger/ })
    .getByRole("button", { name: /upload/i })
    .click();

  const uploadDialog2 = page.getByRole("dialog");
  await expect(uploadDialog2).toBeVisible();

  // Switch format to CAMT.053 (use combobox role to avoid "Date format" input clash)
  await uploadDialog2.getByRole("combobox", { name: "Format" }).click();
  await page.getByRole("option", { name: /CAMT/i }).click();

  // Attach the XML fixture
  await uploadDialog2.getByLabel("File").setInputFiles(LEDGER_XML);

  await uploadDialog2.getByRole("button", { name: /^upload$/i }).click();

  // Success toast: "1 transaction ingested."
  await expect(page.getByText(/1 transaction ingested/i)).toBeVisible({
    timeout: 15_000,
  });

  await expect(uploadDialog2).not.toBeVisible({ timeout: 10_000 });

  // ── Create a run over the two sources ────────────────────────────────────
  await page.goto("/runs");
  await page.getByRole("button", { name: /new run/i }).click();

  const runDialog = page.getByRole("dialog");
  await expect(runDialog).toBeVisible();

  await runDialog.getByLabel("Name").fill("E2E Run");

  await runDialog.getByLabel("Source A").click();
  await page.getByRole("option", { name: "E2E Bank" }).click();

  await runDialog.getByLabel("Source B").click();
  await page.getByRole("option", { name: "E2E Ledger" }).click();

  await runDialog.getByLabel("From").fill("2026-05-01");
  await runDialog.getByLabel("To").fill("2026-05-31");

  await runDialog.getByRole("button", { name: /create run/i }).click();

  // After run creation the app navigates to the run detail page
  await expect(page).toHaveURL(/\/runs\/run-/, { timeout: 20_000 });
});

test("admin creates an MT940 source with subfielded dialect and ingests", async ({
  page,
}) => {
  await loginViaUI(page, ADA_EMAIL, PASSWORD);

  // ── Create an MT940 bank source with subfielded dialect ──────────────────
  await page.goto("/sources");
  await page.getByRole("button", { name: /new source/i }).click();

  const newSourceDialog = page.getByRole("dialog");
  await expect(newSourceDialog).toBeVisible();

  await newSourceDialog.getByLabel("Name").fill("MT940 Acme E2E");
  await newSourceDialog.getByLabel("Currency").fill("GBP");

  // Dialect select is a Base UI Select (combobox role). Click trigger, then option.
  await newSourceDialog
    .getByRole("combobox", { name: /mt940 dialect/i })
    .click();
  await page.getByRole("option", { name: /Subfielded/i }).click();

  await newSourceDialog
    .getByRole("button", { name: /create source/i })
    .click();

  // Dialog closes; source row appears with the dialect badge
  await expect(newSourceDialog).not.toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("cell", { name: /MT940 Acme E2E/ })).toBeVisible();
  await expect(page.getByText(/MT940 · Subfielded/i)).toBeVisible({
    timeout: 10_000,
  });

  // ── Upload an MT940 file into the new source ─────────────────────────────
  // Scope to the new row to avoid clashing with other rows' Upload buttons.
  await page
    .getByRole("row", { name: /MT940 Acme E2E/ })
    .getByRole("button", { name: /upload/i })
    .click();

  const uploadDialog = page.getByRole("dialog");
  await expect(uploadDialog).toBeVisible();

  // Switch format to MT940 via the combobox.
  await uploadDialog.getByRole("combobox", { name: "Format" }).click();
  await page.getByRole("option", { name: /^MT940/i }).click();

  // Attach an MT940 statement (subfielded fixture from backend integration tests).
  const mt940Body =
    ":20:REF20250601\n" +
    ":25:GB29NWBK60161331926819\n" +
    ":28C:00123/00001\n" +
    ":60F:C250601GBP1000,00\n" +
    ":61:250601D100,00NTRFBANKREF-1//BNKREF-A\n" +
    ":86:Counterparty payment\n" +
    ":62F:C250601GBP900,00\n";

  await uploadDialog.getByLabel("File").setInputFiles({
    name: "acme.sta",
    mimeType: "application/octet-stream",
    buffer: Buffer.from(mt940Body, "utf-8"),
  });

  await uploadDialog.getByRole("button", { name: /^upload$/i }).click();

  // Success toast: "1 transaction ingested." (matches upload-dialog onSuccess)
  await expect(page.getByText(/1 transaction ingested/i)).toBeVisible({
    timeout: 15_000,
  });

  // Dialog closes after success
  await expect(uploadDialog).not.toBeVisible({ timeout: 10_000 });
});
