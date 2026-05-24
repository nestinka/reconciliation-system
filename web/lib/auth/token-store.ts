let accessToken: string | null = null;
let refreshFn: (() => Promise<string | null>) | null = null;

export function getAccessToken(): string | null {
  return accessToken;
}

export function setAccessToken(t: string | null): void {
  accessToken = t;
}

export function registerRefresh(fn: (() => Promise<string | null>) | null): void {
  refreshFn = fn;
}

export async function runRefresh(): Promise<string | null> {
  return refreshFn ? refreshFn() : null;
}
