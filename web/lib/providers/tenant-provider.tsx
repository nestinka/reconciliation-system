"use client";

import { createContext, useContext, type ReactNode } from "react";
import { AuthContext } from "@/lib/auth/auth-provider";

interface TenantContextValue {
  tenantId: string;
  setTenantId: (id: string) => void;
}

// Fallback context used by tests that inject a seeded tenant without a full
// AuthProvider (e.g. the TestTenantProvider below).
const TenantContext = createContext<TenantContextValue | null>(null);

/**
 * Kept for backwards-compatibility. No longer manages state; the tenant now
 * comes from AuthProvider. This is a no-op pass-through.
 */
export function TenantProvider({ children }: { children: ReactNode }) {
  return <>{children}</>;
}

export function useTenant(): TenantContextValue {
  // Always call both hooks unconditionally (rules-of-hooks requirement).
  const auth = useContext(AuthContext);
  const fallback = useContext(TenantContext);

  if (auth) {
    return {
      tenantId: auth.activeTenant?.id ?? "tenant-acme",
      setTenantId: () => {
        // Tenant switching is deferred to a later sprint; no-op for now.
      },
    };
  }

  if (fallback) return fallback;

  // Default when neither provider is in the tree.
  return { tenantId: "tenant-acme", setTenantId: () => {} };
}

/**
 * Test helper: wrap children with a seeded TenantContext value so components
 * that call useTenant() outside of AuthProvider get a deterministic tenant.
 */
export function TestTenantProvider({
  tenantId,
  children,
}: {
  tenantId: string;
  children: ReactNode;
}) {
  return (
    <TenantContext.Provider value={{ tenantId, setTenantId: () => {} }}>
      {children}
    </TenantContext.Provider>
  );
}
