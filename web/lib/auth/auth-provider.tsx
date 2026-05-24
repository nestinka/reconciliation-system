"use client";

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import type { User, Tenant, Membership } from "@/lib/domain/types";
import {
  loginRequest,
  refreshRequest,
  logoutRequest,
  switchTenantRequest,
  changePasswordRequest,
  type LoginResult,
} from "./api";
import {
  getAccessToken,
  setAccessToken,
  registerRefresh,
} from "./token-store";

// ---------------------------------------------------------------------------
// JWT helpers (no verification — client-side decode only)
// ---------------------------------------------------------------------------

interface JwtPayload {
  sub?: string;
  exp?: number;
  tid?: string;
  role?: string;
}

function decodeJwtPayload(token: string): JwtPayload {
  try {
    const parts = token.split(".");
    if (parts.length < 2) return {};
    const payload = parts[1];
    // Pad base64url to base64
    const padded = payload.replace(/-/g, "+").replace(/_/g, "/");
    const padLength = (4 - (padded.length % 4)) % 4;
    const base64 = padded + "=".repeat(padLength);
    return JSON.parse(atob(base64)) as JwtPayload;
  } catch {
    return {};
  }
}

// ---------------------------------------------------------------------------
// Session shape persisted in localStorage (non-secret)
// ---------------------------------------------------------------------------

const SESSION_STORAGE_KEY = "recon:session";

interface PersistedSession {
  user: User;
  activeTenant: Tenant;
  memberships: Membership[];
}

function loadSession(): PersistedSession | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(SESSION_STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as PersistedSession;
  } catch {
    return null;
  }
}

function saveSession(session: PersistedSession): void {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(SESSION_STORAGE_KEY, JSON.stringify(session));
}

function clearSession(): void {
  if (typeof window === "undefined") return;
  window.localStorage.removeItem(SESSION_STORAGE_KEY);
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

export type AuthStatus = "loading" | "authenticated" | "unauthenticated";

export interface AuthContextValue {
  status: AuthStatus;
  user: User | null;
  memberships: Membership[];
  activeTenant: Tenant | null;
  login: (email: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
  switchTenant: (tenantId: string) => Promise<void>;
  changePassword: (currentPassword: string, newPassword: string) => Promise<void>;
}

export const AuthContext = createContext<AuthContextValue | null>(null);

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>("loading");
  const [user, setUser] = useState<User | null>(null);
  const [memberships, setMemberships] = useState<Membership[]>([]);
  const [activeTenant, setActiveTenant] = useState<Tenant | null>(null);

  const tokenRef = useRef<string | null>(null);
  const refreshTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // -------------------------------------------------------------------------
  // Helpers
  // -------------------------------------------------------------------------

  function applyToken(token: string) {
    tokenRef.current = token;
    setAccessToken(token);
  }

  function clearToken() {
    tokenRef.current = null;
    setAccessToken(null);
  }

  // scheduleRefreshFromToken is defined after doRefresh to avoid forward reference issues.

  const doRefresh = useCallback(async (): Promise<string | null> => {
    const token = await refreshRequest();
    if (!token) {
      clearToken();
      clearSession();
      setUser(null);
      setMemberships([]);
      setActiveTenant(null);
      setStatus("unauthenticated");
      return null;
    }
    applyToken(token);
    // Schedule next refresh after a successful one
    scheduleRefreshFromToken(token);
    return token;
  }, []);

  function scheduleRefreshFromToken(token: string) {
    if (refreshTimerRef.current) {
      clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = null;
    }
    const { exp } = decodeJwtPayload(token);
    if (!exp) return;
    const nowSec = Math.floor(Date.now() / 1000);
    // Refresh 60 seconds before expiry, minimum 5 seconds from now
    const delayMs = Math.max((exp - nowSec - 60) * 1000, 5000);
    refreshTimerRef.current = setTimeout(() => {
      void doRefresh();
    }, delayMs);
  }

  // -------------------------------------------------------------------------
  // Bootstrap: try to restore session from cookie + localStorage
  // -------------------------------------------------------------------------

  useEffect(() => {
    let cancelled = false;

    async function bootstrap() {
      const token = await refreshRequest();
      if (cancelled) return;

      if (!token) {
        clearSession();
        setStatus("unauthenticated");
        return;
      }

      applyToken(token);
      scheduleRefreshFromToken(token);

      const session = loadSession();
      if (session) {
        setUser(session.user);
        setMemberships(session.memberships);
        setActiveTenant(session.activeTenant);
      }

      setStatus("authenticated");
    }

    void bootstrap();
    return () => {
      cancelled = true;
    };
  }, []);

  // -------------------------------------------------------------------------
  // Register the refresh function in the token store so HttpApiClient can call it
  // -------------------------------------------------------------------------

  useEffect(() => {
    registerRefresh(doRefresh);
    return () => {
      registerRefresh(null);
    };
  }, [doRefresh]);

  // -------------------------------------------------------------------------
  // Cleanup timer on unmount
  // -------------------------------------------------------------------------

  useEffect(() => {
    return () => {
      if (refreshTimerRef.current) {
        clearTimeout(refreshTimerRef.current);
      }
    };
  }, []);

  // -------------------------------------------------------------------------
  // Actions
  // -------------------------------------------------------------------------

  const login = useCallback(async (email: string, password: string) => {
    const result: LoginResult = await loginRequest(email, password);
    applyToken(result.accessToken);
    scheduleRefreshFromToken(result.accessToken);

    const session: PersistedSession = {
      user: result.user,
      activeTenant: result.activeTenant,
      memberships: result.memberships,
    };
    saveSession(session);

    setUser(result.user);
    setMemberships(result.memberships);
    setActiveTenant(result.activeTenant);
    setStatus("authenticated");
  }, []);

  const logout = useCallback(async () => {
    try {
      await logoutRequest();
    } catch {
      // best-effort
    }
    clearToken();
    clearSession();
    if (refreshTimerRef.current) {
      clearTimeout(refreshTimerRef.current);
      refreshTimerRef.current = null;
    }
    setUser(null);
    setMemberships([]);
    setActiveTenant(null);
    setStatus("unauthenticated");
  }, []);

  const switchTenant = useCallback(async (tenantId: string) => {
    const newToken = await switchTenantRequest(tenantId);
    applyToken(newToken);
    scheduleRefreshFromToken(newToken);

    // Find the matching membership to build the tenant object and get the role
    const membership = memberships.find((m) => m.tenantId === tenantId);
    const newTenant: Tenant = membership
      ? { id: membership.tenantId, name: membership.tenantName, slug: "" }
      : { id: tenantId, name: tenantId, slug: "" };

    // Update the user's role for the new tenant if we found the membership
    const updatedUser = user && membership
      ? { ...user, role: membership.role }
      : user;

    const updatedSession: PersistedSession = {
      user: updatedUser ?? (user as User),
      activeTenant: newTenant,
      memberships,
    };

    saveSession(updatedSession);
    setActiveTenant(newTenant);
    if (updatedUser) setUser(updatedUser);
  }, [memberships, user]);

  const changePassword = useCallback(async (currentPassword: string, newPassword: string) => {
    await changePasswordRequest(currentPassword, newPassword);
  }, []);

  const value: AuthContextValue = {
    status,
    user,
    memberships,
    activeTenant,
    login,
    logout,
    switchTenant,
    changePassword,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth must be used inside <AuthProvider>");
  return ctx;
}

// ---------------------------------------------------------------------------
// Re-export getAccessToken for convenience (used in test helpers)
// ---------------------------------------------------------------------------
export { getAccessToken };
