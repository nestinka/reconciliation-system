"use client";

import { createContext, useContext, type ReactNode } from "react";
import { AuthContext } from "@/lib/auth/auth-provider";

interface CurrentUserContextValue {
  currentUserId: string;
  setCurrentUserId: (id: string) => void;
}

// Fallback context used by tests that inject a seeded user without a full
// AuthProvider (e.g. the TestCurrentUserProvider below).
const CurrentUserContext = createContext<CurrentUserContextValue | null>(null);

/**
 * Kept for backwards-compatibility. No longer manages state; the current user
 * now comes from AuthProvider. This is a no-op pass-through.
 */
export function CurrentUserProvider({ children }: { children: ReactNode }) {
  return <>{children}</>;
}

export function useCurrentUserId(): CurrentUserContextValue {
  // Always call both hooks unconditionally (rules-of-hooks requirement).
  const auth = useContext(AuthContext);
  const fallback = useContext(CurrentUserContext);

  if (auth) {
    return {
      currentUserId: auth.user?.id ?? "",
      setCurrentUserId: () => {
        // User switching is no longer supported via this hook; no-op.
      },
    };
  }

  if (fallback) return fallback;

  // Default when neither provider is in the tree.
  return { currentUserId: "", setCurrentUserId: () => {} };
}

/**
 * Test helper: wrap children with a seeded CurrentUserContext value so
 * components that call useCurrentUserId() outside of AuthProvider get a
 * deterministic user.
 */
export function TestCurrentUserProvider({
  currentUserId,
  children,
}: {
  currentUserId: string;
  children: ReactNode;
}) {
  return (
    <CurrentUserContext.Provider
      value={{ currentUserId, setCurrentUserId: () => {} }}
    >
      {children}
    </CurrentUserContext.Provider>
  );
}
