"use client";

import { createContext, useContext, type ReactNode } from "react";
import type { ApiClient } from "./client";
import { HttpApiClient } from "./http";

const ApiContext = createContext<ApiClient | null>(null);

const API_BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";
const defaultClient: ApiClient = new HttpApiClient(API_BASE_URL);

export function ApiProvider({
  client = defaultClient,
  children,
}: {
  client?: ApiClient;
  children: ReactNode;
}) {
  return <ApiContext.Provider value={client}>{children}</ApiContext.Provider>;
}

export function useApi(): ApiClient {
  const ctx = useContext(ApiContext);
  if (!ctx) throw new Error("useApi must be used inside <ApiProvider>");
  return ctx;
}
