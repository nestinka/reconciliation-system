"use client";

import { createContext, useContext, type ReactNode } from "react";
import { usePersistedState } from "@/lib/hooks/use-persisted-state";

const STORAGE_KEY = "recon:activeTenantId";
const DEFAULT_TENANT = "tenant-acme";

interface TenantContextValue {
  tenantId: string;
  setTenantId: (id: string) => void;
}

const TenantContext = createContext<TenantContextValue | null>(null);

export function TenantProvider({ children }: { children: ReactNode }) {
  const [tenantId, setTenantId] = usePersistedState(STORAGE_KEY, DEFAULT_TENANT);

  return (
    <TenantContext.Provider value={{ tenantId, setTenantId }}>
      {children}
    </TenantContext.Provider>
  );
}

export function useTenant(): TenantContextValue {
  const ctx = useContext(TenantContext);
  if (!ctx) {
    throw new Error("useTenant must be used inside <TenantProvider>");
  }
  return ctx;
}
