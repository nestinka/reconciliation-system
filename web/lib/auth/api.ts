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
