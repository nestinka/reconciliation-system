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
import { EditSourceDialog } from "@/components/app/edit-source-dialog";
import type { ApiClient } from "@/lib/api/client";
import type { Source } from "@/lib/domain/types";

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

const BASE_SOURCE: Source = {
  id: "src-1",
  tenantId: "tenant-acme",
  kind: "bank",
  name: "Bank A",
  currency: "EUR",
  formatDialect: null,
};

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, staleTime: 0 } },
  });
}

function renderDialog(
  client: ApiClient,
  source: Source = BASE_SOURCE,
  open = true,
  onOpenChange = vi.fn(),
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
    <EditSourceDialog source={source} open={open} onOpenChange={onOpenChange} />,
    { wrapper: Wrapper },
  );
}

describe("EditSourceDialog", () => {
  it("renders pre-filled name and dialect", async () => {
    const client = new MockApiClient({ latencyMs: 0 });
    renderDialog(client, { ...BASE_SOURCE, formatDialect: "subfielded" });

    expect(screen.getByLabelText(/^name$/i)).toHaveValue("Bank A");
    // The trigger shows the current value — "Subfielded" should appear
    expect(screen.getByText(/subfielded/i)).toBeInTheDocument();
  });

  it("submits and calls api.updateSource with changed name", async () => {
    const user = userEvent.setup();
    const base = new MockApiClient({ latencyMs: 0 });
    const updateSpy = vi.fn(base.updateSource.bind(base));
    const stubClient: ApiClient = Object.assign(base, {
      updateSource: updateSpy,
    });

    renderDialog(stubClient);

    const nameInput = screen.getByLabelText(/^name$/i);
    await user.clear(nameInput);
    await user.type(nameInput, "Bank A renamed");
    await user.click(screen.getByRole("button", { name: /save/i }));

    await waitFor(() => {
      expect(updateSpy).toHaveBeenCalled();
    });
    expect(updateSpy).toHaveBeenCalledWith(
      "tenant-acme",
      "src-1",
      expect.objectContaining({ name: "Bank A renamed" }),
    );
  });

  it("Cancel closes the dialog without calling api", async () => {
    const user = userEvent.setup();
    const client = new MockApiClient({ latencyMs: 0 });
    const updateSpy = vi.spyOn(client, "updateSource");
    const onOpenChange = vi.fn();

    renderDialog(client, BASE_SOURCE, true, onOpenChange);

    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(updateSpy).not.toHaveBeenCalled();
  });
});
