"use client";

import {
  createContext,
  useContext,
  useState,
  type ReactNode,
} from "react";

const STORAGE_KEY = "recon:currentUserId";
const DEFAULT_USER = "user-mia";

interface CurrentUserContextValue {
  currentUserId: string;
  setCurrentUserId: (id: string) => void;
}

const CurrentUserContext = createContext<CurrentUserContextValue | null>(null);

export function CurrentUserProvider({ children }: { children: ReactNode }) {
  const [currentUserId, setCurrentUserIdState] = useState<string>(() => {
    if (typeof window !== "undefined") {
      return localStorage.getItem(STORAGE_KEY) ?? DEFAULT_USER;
    }
    return DEFAULT_USER;
  });

  function setCurrentUserId(id: string) {
    setCurrentUserIdState(id);
    if (typeof window !== "undefined") {
      localStorage.setItem(STORAGE_KEY, id);
    }
  }

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
