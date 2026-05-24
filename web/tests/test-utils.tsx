import React, { type ReactElement } from "react";
import { render, type RenderResult } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { NuqsTestingAdapter } from "nuqs/adapters/testing";
import { ThemeProvider } from "next-themes";
import { ApiProvider } from "@/lib/api/provider";
import { MockApiClient } from "@/lib/api/mock";
import { MockAuthProvider } from "@/lib/auth/mock-auth-provider";
import type { User, Tenant, Membership } from "@/lib/domain/types";

export { screen, waitFor, within, act } from "@testing-library/react";
export { default as userEvent } from "@testing-library/user-event";

// Re-export legacy provider names so existing test files that import them
// still compile. The providers are now no-op stubs but the import is valid.
export { TenantProvider } from "@/lib/providers/tenant-provider";
export { CurrentUserProvider } from "@/lib/providers/current-user-provider";

export interface RenderOptions {
  /** Override which user is active (by id). Populates useCurrentUserId(). */
  currentUserId?: string;
  /** Override which tenant is active (by id). Populates useTenant(). */
  tenantId?: string;
  /** Seed memberships for the session (used by TenantSwitcher etc.). */
  memberships?: Membership[];
  /** Pre-seed URL search params for nuqs filters (e.g. "?type=duplicate" or { type: "duplicate" }). */
  searchParams?: string | Record<string, string>;
}

// Minimal fixture data so the mock auth session can look up names from ids.
const FIXTURE_USERS: Record<string, User> = {
  "user-mia":  { id: "user-mia",  name: "Mia",  role: "operator" },
  "user-sam":  { id: "user-sam",  name: "Sam",  role: "operator" },
  "user-theo": { id: "user-theo", name: "Theo", role: "approver" },
  "user-ada":  { id: "user-ada",  name: "Ada",  role: "admin" },
};

const FIXTURE_TENANTS: Record<string, Tenant> = {
  "tenant-acme":   { id: "tenant-acme",   name: "Acme Capital",   slug: "acme-capital" },
  "tenant-globex": { id: "tenant-globex", name: "Globex Markets", slug: "globex-markets" },
};

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

  const userId = options.currentUserId ?? "user-mia";
  const tenantId = options.tenantId ?? "tenant-acme";

  const user = FIXTURE_USERS[userId] ?? { id: userId, name: userId, role: "operator" as const };
  const tenant = FIXTURE_TENANTS[tenantId] ?? { id: tenantId, name: tenantId, slug: tenantId };

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

  function Wrapper({ children }: { children: React.ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>
        <MockAuthProvider session={{ user, activeTenant: tenant, memberships: options.memberships ?? [] }}>
          <ApiProvider client={mockClient}>
            <ThemeProvider attribute="class" defaultTheme="light" enableSystem={false}>
              <NuqsTestingAdapter searchParams={nuqsSearchParams}>
                {children}
              </NuqsTestingAdapter>
            </ThemeProvider>
          </ApiProvider>
        </MockAuthProvider>
      </QueryClientProvider>
    );
  }

  const result = render(ui, { wrapper: Wrapper });
  return { ...result, queryClient };
}
