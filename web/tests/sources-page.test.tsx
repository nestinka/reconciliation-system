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
import SourcesPage from "@/app/(app)/sources/page";
import type { ApiClient } from "@/lib/api/client";

// next/navigation is referenced indirectly by some children — stub it.
vi.mock("next/navigation", () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => "/sources",
}));

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

function renderSourcesPage(client: ApiClient) {
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

  return render(<SourcesPage />, { wrapper: Wrapper });
}

describe("New source dialog — MT940 dialect", () => {
  it("renders the MT940 dialect select inside the dialog", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(client);

    // Open the dialog.
    await user.click(
      await screen.findByRole("button", { name: /new source/i })
    );

    // The dialect select trigger renders inside the dialog. Base UI's Select
    // exposes the trigger as a combobox with the aria-label we set.
    const trigger = await screen.findByRole("combobox", {
      name: /mt940 dialect/i,
    });
    expect(trigger).toBeInTheDocument();

    // The descriptive helper text is also rendered.
    expect(
      screen.getByText(/Set this only if this source will receive MT940/i)
    ).toBeInTheDocument();
    expect(screen.getByText(/Subfielded/i)).toBeInTheDocument();
  });

  it("submits formatDialect=subfielded when the user picks Subfielded", async () => {
    const user = userEvent.setup();

    const base = new MockApiClient({ latencyMs: 0 });
    const createSpy = vi.fn(base.createSource.bind(base));
    const stubClient: ApiClient = Object.assign(base, {
      createSource: createSpy,
    });

    renderSourcesPage(stubClient);

    // Open the dialog.
    await user.click(
      await screen.findByRole("button", { name: /new source/i })
    );

    // Fill name + currency.
    await user.type(screen.getByLabelText(/^name/i), "MT940 Bank");

    const ccy = screen.getByLabelText(/^currency/i) as HTMLInputElement;
    await user.clear(ccy);
    await user.type(ccy, "GBP");

    // Open the dialect select and choose Subfielded.
    await user.click(
      screen.getByRole("combobox", { name: /mt940 dialect/i })
    );
    await user.click(
      await screen.findByRole("option", { name: /subfielded/i })
    );

    // Submit the form.
    await user.click(
      screen.getByRole("button", { name: /create source/i })
    );

    await waitFor(() => {
      expect(createSpy).toHaveBeenCalled();
    });
    expect(createSpy).toHaveBeenCalledWith(
      "tenant-acme",
      expect.objectContaining({
        kind: "bank",
        name: "MT940 Bank",
        currency: "GBP",
        formatDialect: "subfielded",
      })
    );
  });
});

describe("Sources table — Edit button", () => {
  it("renders an Edit button on each row", async () => {
    const client = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(client);

    // Wait for rows to load; the fixture has at least one source for tenant-acme.
    const editButtons = await screen.findAllByRole("button", { name: /^edit$/i });
    expect(editButtons.length).toBeGreaterThan(0);
  });

  it("clicking Edit opens the EditSourceDialog", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(client);

    const editButtons = await screen.findAllByRole("button", { name: /^edit$/i });
    await user.click(editButtons[0]);

    expect(
      await screen.findByRole("heading", { name: /edit source/i }),
    ).toBeVisible();
  });
});

describe("Sources table — dialect badge", () => {
  it("renders the MT940 dialect badge for sources with formatDialect set", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(client);

    // Create a new MT940 / Subfielded source via the dialog.
    await user.click(
      await screen.findByRole("button", { name: /new source/i })
    );

    await user.type(screen.getByLabelText(/^name/i), "MT940 Acme");

    const ccy = screen.getByLabelText(/^currency/i) as HTMLInputElement;
    await user.clear(ccy);
    await user.type(ccy, "GBP");

    await user.click(
      screen.getByRole("combobox", { name: /mt940 dialect/i })
    );
    await user.click(
      await screen.findByRole("option", { name: /subfielded/i })
    );

    await user.click(
      screen.getByRole("button", { name: /create source/i })
    );

    // The new row appears in the table with the dialect badge.
    await waitFor(() => {
      expect(screen.getByText("MT940 Acme")).toBeInTheDocument();
    });
    expect(screen.getByText(/MT940 · Subfielded/i)).toBeInTheDocument();
  });

  it("does NOT render the badge for sources without formatDialect", async () => {
    const client = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(client);

    // The mock's seed sources all have formatDialect: null. Wait for the
    // table to populate, then assert no MT940 badge is present.
    await waitFor(() => {
      expect(screen.getByText("Acme Bank Statement")).toBeInTheDocument();
    });
    expect(screen.queryByText(/MT940 ·/i)).not.toBeInTheDocument();
  });
});
