import React, { type ReactElement } from "react";
import { render, type RenderResult } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { NuqsTestingAdapter } from "nuqs/adapters/testing";
import { ThemeProvider } from "next-themes";
import { ApiProvider } from "@/lib/api/provider";
import { MockApiClient } from "@/lib/api/mock";
import { TenantProvider } from "@/lib/providers/tenant-provider";
import { CurrentUserProvider } from "@/lib/providers/current-user-provider";

export { screen, waitFor, within, act } from "@testing-library/react";
export { default as userEvent } from "@testing-library/user-event";

export interface RenderOptions {
  /** Pre-seed localStorage with a specific tenantId before rendering. */
  tenantId?: string;
  /** Pre-seed URL search params for nuqs filters (e.g. "?type=duplicate" or { type: "duplicate" }). */
  searchParams?: string | Record<string, string>;
}

export function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
        staleTime: 0,
      },
    },
  });
}

export function renderWithProviders(
  ui: ReactElement,
  options: RenderOptions = {}
): RenderResult & { queryClient: QueryClient } {
  const queryClient = makeQueryClient();
  const mockClient = new MockApiClient({ latencyMs: 0 });

  // Pre-seed the tenant if caller requests a specific one.
  if (options.tenantId) {
    window.localStorage.setItem("recon:activeTenantId", options.tenantId);
  }

  // Normalise searchParams: NuqsTestingAdapter accepts Record<string, string>.
  let nuqsSearchParams: Record<string, string> | undefined;
  if (options.searchParams) {
    if (typeof options.searchParams === "string") {
      const sp = new URLSearchParams(options.searchParams.replace(/^\?/, ""));
      nuqsSearchParams = Object.fromEntries(sp.entries());
    } else {
      nuqsSearchParams = options.searchParams;
    }
  }

  // Wrap providers: QueryClient > ApiProvider > TenantProvider > CurrentUserProvider > ThemeProvider > NuqsTestingAdapter
  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <ApiProvider client={mockClient}>
          <TenantProvider>
            <CurrentUserProvider>
              <ThemeProvider attribute="class" defaultTheme="light" enableSystem={false}>
                <NuqsTestingAdapter searchParams={nuqsSearchParams}>
                  {children}
                </NuqsTestingAdapter>
              </ThemeProvider>
            </CurrentUserProvider>
          </TenantProvider>
        </ApiProvider>
      </QueryClientProvider>
    );
  }

  const result = render(ui, { wrapper: Wrapper });
  return { ...result, queryClient };
}
