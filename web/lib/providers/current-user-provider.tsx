"use client";

import { createContext, useContext, type ReactNode } from "react";
import { usePersistedState } from "@/lib/hooks/use-persisted-state";

const STORAGE_KEY = "recon:currentUserId";
const DEFAULT_USER = "user-mia";

interface CurrentUserContextValue {
  currentUserId: string;
  setCurrentUserId: (id: string) => void;
}

const CurrentUserContext = createContext<CurrentUserContextValue | null>(null);

export function CurrentUserProvider({ children }: { children: ReactNode }) {
  const [currentUserId, setCurrentUserId] = usePersistedState(
    STORAGE_KEY,
    DEFAULT_USER
  );

  return (
    <CurrentUserContext.Provider value={{ currentUserId, setCurrentUserId }}>
      {children}
    </CurrentUserContext.Provider>
  );
}

export function useCurrentUserId(): CurrentUserContextValue {
  const ctx = useContext(CurrentUserContext);
  if (!ctx) {
    throw new Error(
      "useCurrentUserId must be used inside <CurrentUserProvider>"
    );
  }
  return ctx;
}
