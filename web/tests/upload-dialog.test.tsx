import { describe, it, expect, vi } from "vitest";
import React from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { NuqsTestingAdapter } from "nuqs/adapters/testing";
import { ThemeProvider } from "next-themes";
import { ApiProvider } from "@/lib/api/provider";
import { MockApiClient } from "@/lib/api/mock";
import { MockAuthProvider } from "@/lib/auth/mock-auth-provider";
import { IngestError } from "@/lib/api/client";
import { UploadDialog } from "@/components/app/upload-dialog";
import type { SourceListItem } from "@/lib/api/client";
import type { ApiClient } from "@/lib/api/client";

// Mock next/navigation since the component tree may reference useRouter
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/sources",
}));

const MOCK_SOURCE: SourceListItem = {
  id: "src-test",
  tenantId: "tenant-acme",
  kind: "bank",
  name: "Test Bank",
  currency: "GBP",
  formatDialect: null,
  txnCount: 0,
};

const FIXTURE_USER = { id: "user-ada", name: "Ada", role: "admin" as const };
const FIXTURE_TENANT = {
  id: "tenant-acme",
  name: "Acme Capital",
  slug: "acme-capital",
};

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
}

function renderDialog(
  client: ApiClient,
  open = true,
  onOpenChange = vi.fn()
) {
  const queryClient = makeQueryClient();

  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <MockAuthProvider
          session={{
            user: FIXTURE_USER,
            activeTenant: FIXTURE_TENANT,
            memberships: [],
          }}
        >
          <ApiProvider client={client}>
            <ThemeProvider
              attribute="class"
              defaultTheme="light"
              enableSystem={false}
            >
              <NuqsTestingAdapter>{children}</NuqsTestingAdapter>
            </ThemeProvider>
          </ApiProvider>
        </MockAuthProvider>
      </QueryClientProvider>
    );
  }

  return render(
    <UploadDialog
      source={MOCK_SOURCE}
      open={open}
      onOpenChange={onOpenChange}
    />,
    { wrapper: Wrapper }
  );
}

describe("UploadDialog", () => {
  it("renders the dialog with source name", () => {
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);
    expect(screen.getByText(/Upload to Test Bank/i)).toBeInTheDocument();
  });

  it("Upload button is disabled when no file is selected", () => {
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);
    const uploadBtn = screen.getByRole("button", { name: /^upload$/i });
    expect(uploadBtn).toBeDisabled();
  });

  it("shows parse error report when ingestFile throws IngestError(parse)", async () => {
    const user = userEvent.setup();

    // Stub client whose ingestFile rejects with an IngestError
    const base = new MockApiClient({ latencyMs: 0 });
    const stubClient: ApiClient = Object.assign(base, {
      ingestFile: vi.fn().mockRejectedValue(
        new IngestError("parse", "file contains invalid rows", [
          { row: 4, field: "valueDate", message: "unparseable" },
        ])
      ),
    });

    renderDialog(stubClient);

    // Select a file
    const fileInput = screen.getByLabelText(/file/i);
    const testFile = new File(["ref,date\nR1,bad"], "test.csv", {
      type: "text/csv",
    });
    await user.upload(fileInput, testFile);

    // Click Upload
    const uploadBtn = screen.getByRole("button", { name: /^upload$/i });
    await user.click(uploadBtn);

    // Error report should appear
    expect(await screen.findByText(/fix these rows/i)).toBeInTheDocument();
    expect(screen.getByText(/Row 4: valueDate/)).toBeInTheDocument();
  });

  it("shows duplicate refs report when ingestFile throws IngestError(duplicate)", async () => {
    const user = userEvent.setup();

    const base2 = new MockApiClient({ latencyMs: 0 });
    const stubClient: ApiClient = Object.assign(base2, {
      ingestFile: vi.fn().mockRejectedValue(
        new IngestError("duplicate", "duplicate transaction references", undefined, [
          "REF-001",
          "REF-002",
        ])
      ),
    });

    renderDialog(stubClient);

    const fileInput = screen.getByLabelText(/file/i);
    const testFile = new File(["data"], "test.csv", { type: "text/csv" });
    await user.upload(fileInput, testFile);

    const uploadBtn = screen.getByRole("button", { name: /^upload$/i });
    await user.click(uploadBtn);

    expect(
      await screen.findByText(/Duplicate references already loaded/i)
    ).toBeInTheDocument();
    expect(screen.getByText(/REF-001/)).toBeInTheDocument();
  });

  it("closes dialog and shows toast on successful ingest", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();

    const client = new MockApiClient({ latencyMs: 0 });

    renderDialog(client, true, onOpenChange);

    const fileInput = screen.getByLabelText(/file/i);
    const testFile = new File(["data"], "test.csv", { type: "text/csv" });
    await user.upload(fileInput, testFile);

    const uploadBtn = screen.getByRole("button", { name: /^upload$/i });
    await user.click(uploadBtn);

    await waitFor(() => {
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });

  it("shows MT940 and BAI2 in the format dropdown", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);

    // Open the Shadcn Select to surface the SelectItem options.
    await user.click(screen.getByRole("combobox", { name: /format/i }));

    expect(
      await screen.findByRole("option", { name: /CSV \(with column mapping\)/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /CAMT\.053/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /MT940/i })
    ).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /BAI v2/i })
    ).toBeInTheDocument();
  });

  it("hides the CSV mapping form when MT940 is selected", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);

    // CSV mapping form is initially visible.
    expect(screen.getByLabelText(/reference col/i)).toBeInTheDocument();

    // Switch format to MT940.
    await user.click(screen.getByRole("combobox", { name: /format/i }));
    await user.click(await screen.findByRole("option", { name: /MT940/i }));

    // CSV mapping fields should no longer be in the DOM.
    expect(screen.queryByLabelText(/reference col/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/amount col/i)).not.toBeInTheDocument();
  });

  it("shows notice when source has no dialect and format is MT940", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    // MOCK_SOURCE.formatDialect is null — the notice should appear.
    renderDialog(client);

    await user.click(screen.getByRole("combobox", { name: /format/i }));
    await user.click(await screen.findByRole("option", { name: /MT940/i }));

    expect(
      screen.getByText(/MT940\/MT942 dialect/i)
    ).toBeInTheDocument();
  });

  it("offers MT942 as the fifth format option", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);
    await user.click(screen.getByRole("combobox", { name: /format/i }));
    expect(screen.getByText(/MT942 \(intra-day\)/i)).toBeInTheDocument();
  });

  it("hides CSV mapping fields when MT942 is selected", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);
    await user.click(screen.getByRole("combobox", { name: /format/i }));
    await user.click(await screen.findByRole("option", { name: /MT942/i }));
    expect(screen.queryByLabelText(/reference col|amount col/i)).not.toBeInTheDocument();
  });

  it("shows the amber dialect-missing notice for MT942 when source has no dialect", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client);
    await user.click(screen.getByRole("combobox", { name: /format/i }));
    await user.click(await screen.findByRole("option", { name: /MT942/i }));
    expect(screen.getByText(/MT940\/MT942 dialect|dialect/i)).toBeInTheDocument();
  });
});
