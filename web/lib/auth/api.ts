import type { User, Tenant, UserRole } from "@/lib/domain/types";

const BASE = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";

export interface Membership {
  tenantId: string;
  tenantName: string;
  role: UserRole;
}

export interface LoginResult {
  accessToken: string;
  user: User;
  activeTenant: Tenant;
  memberships: Membership[];
}

export class AuthError extends Error {
  constructor(
    message: string,
    public readonly status: number
  ) {
    super(message);
    this.name = "AuthError";
  }
}

export async function loginRequest(
  email: string,
  password: string
): Promise<LoginResult> {
  const res = await fetch(`${BASE}/auth/login`, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
  });
  if (!res.ok) {
    let message = `Login failed: ${res.status}`;
    try {
      const b = await res.json();
      message = b?.error?.message ?? b?.message ?? message;
    } catch {
      // ignore
    }
    throw new AuthError(message, res.status);
  }
  return res.json() as Promise<LoginResult>;
}

export async function refreshRequest(): Promise<string | null> {
  const res = await fetch(`${BASE}/auth/refresh`, {
    method: "POST",
    credentials: "include",
  });
  if (!res.ok) {
    return null;
  }
  const data = (await res.json()) as { accessToken: string };
  return data.accessToken ?? null;
}

export async function logoutRequest(): Promise<void> {
  await fetch(`${BASE}/auth/logout`, {
    method: "POST",
    credentials: "include",
  });
}

export async function switchTenantRequest(tenantId: string): Promise<string> {
  const { getAccessToken } = await import("@/lib/auth/token-store");
  const token = getAccessToken();
  const res = await fetch(`${BASE}/auth/switch-tenant`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ tenantId }),
  });
  if (!res.ok) {
    let message = `Switch tenant failed: ${res.status}`;
    try {
      const b = await res.json();
      message = b?.error?.message ?? b?.message ?? message;
    } catch {
      // ignore
    }
    throw new AuthError(message, res.status);
  }
  const data = (await res.json()) as { accessToken: string };
  return data.accessToken;
}

export async function changePasswordRequest(
  currentPassword: string,
  newPassword: string
): Promise<void> {
  const { getAccessToken } = await import("@/lib/auth/token-store");
  const token = getAccessToken();
  const res = await fetch(`${BASE}/auth/password`, {
    method: "POST",
    credentials: "include",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ currentPassword, newPassword }),
  });
  if (!res.ok) {
    let message = `Change password failed: ${res.status}`;
    try {
      const b = await res.json();
      message = b?.error?.message ?? b?.message ?? message;
    } catch {
      // ignore
    }
    throw new AuthError(message, res.status);
  }
}

export async function forgotRequest(email: string): Promise<void> {
  await fetch(`${BASE}/auth/forgot`, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email }),
  });
}

export async function resetRequest(
  token: string,
  newPassword: string
): Promise<void> {
  const res = await fetch(`${BASE}/auth/reset`, {
    method: "POST",
    credentials: "include",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ token, newPassword }),
  });
  if (!res.ok) {
    let message = `Reset password failed: ${res.status}`;
    try {
      const b = await res.json();
      message = b?.error?.message ?? b?.message ?? message;
    } catch {
      // ignore
    }
    throw new AuthError(message, res.status);
  }
}
