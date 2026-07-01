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

describe("Sources table — archive/restore row action", () => {
  it("archives a source via the row action", async () => {
    const user = userEvent.setup();
    const base = new MockApiClient({ latencyMs: 0 });
    const spy = vi.fn(base.archiveSource.bind(base));
    const client: ApiClient = Object.assign(base, { archiveSource: spy });
    renderSourcesPage(client);

    // Wait for table rows — the seed data has three enabled sources for tenant-acme
    const archiveButtons = await screen.findAllByRole("button", {
      name: /^archive$/i,
    });
    expect(archiveButtons.length).toBeGreaterThan(0);

    // Click the first Archive button
    await user.click(archiveButtons[0]);

    await waitFor(() => expect(spy).toHaveBeenCalled());
    expect(spy).toHaveBeenCalledWith("tenant-acme", expect.any(String));
  });

  it("shows 'Show archived' checkbox that is unchecked by default", async () => {
    const client = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(client);

    // Wait for the page to load
    await screen.findAllByRole("button", { name: /^archive$/i });

    const checkbox = screen.getByRole("checkbox", { name: /show archived/i });
    expect(checkbox).toBeInTheDocument();
    expect(checkbox).not.toBeChecked();
  });

  it("renders archived rows with Archived badge when show archived is toggled on", async () => {
    const user = userEvent.setup();
    const base = new MockApiClient({ latencyMs: 0 });
    renderSourcesPage(base);

    // Wait for rows to load and archive the first source
    const archiveButtons = await screen.findAllByRole("button", {
      name: /^archive$/i,
    });
    await user.click(archiveButtons[0]);

    // Wait for the archive to complete (button disappears from default view)
    await waitFor(() => {
      const buttons = screen.queryAllByRole("button", { name: /^archive$/i });
      expect(buttons.length).toBeLessThan(archiveButtons.length);
    });

    // Toggle on "Show archived"
    const checkbox = screen.getByRole("checkbox", { name: /show archived/i });
    await user.click(checkbox);

    // The Archived badge should now appear
    await waitFor(() => {
      expect(screen.getByText("Archived")).toBeInTheDocument();
    });
  });
});
