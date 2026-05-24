"use client";

import { type ReactNode } from "react";
import type { User, Tenant, Membership } from "@/lib/domain/types";
import { AuthContext, type AuthContextValue, type AuthStatus } from "./auth-provider";

export interface MockAuthSession {
  user?: User | null;
  activeTenant?: Tenant | null;
  memberships?: Membership[];
  status?: AuthStatus;
}

const DEFAULT_USER: User = {
  id: "user-mia",
  name: "Mia",
  role: "operator",
};

const DEFAULT_TENANT: Tenant = {
  id: "tenant-acme",
  name: "Acme Capital",
  slug: "acme-capital",
};

/**
 * Drop-in replacement for AuthProvider in tests. Provides a pre-seeded
 * authenticated session without making any network requests.
 */
export function MockAuthProvider({
  session = {},
  children,
}: {
  session?: MockAuthSession;
  children: ReactNode;
}) {
  const value: AuthContextValue = {
    status: session.status ?? "authenticated",
    user: session.user !== undefined ? session.user : DEFAULT_USER,
    activeTenant:
      session.activeTenant !== undefined ? session.activeTenant : DEFAULT_TENANT,
    memberships: session.memberships ?? [],
    login: async () => {},
    logout: async () => {},
  };

  return (
    <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
  );
}
