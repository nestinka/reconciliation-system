"use client";

import { createContext, useContext, type ReactNode } from "react";
import type { ApiClient } from "./client";
import { MockApiClient } from "./mock";

const ApiContext = createContext<ApiClient | null>(null);

const defaultClient = new MockApiClient();

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
  if (!ctx) {
    throw new Error("useApi must be used inside <ApiProvider>");
  }
  return ctx;
}
