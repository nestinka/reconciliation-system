import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { RunQuery } from "@/lib/api/client";

export function useRuns(q?: RunQuery) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["runs", tenantId, q],
    queryFn: () => api.listRuns(tenantId, q),
  });
}

export function useRun(runId: string) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["run", tenantId, runId],
    queryFn: () => api.getRun(tenantId, runId),
    enabled: Boolean(runId),
  });
}
