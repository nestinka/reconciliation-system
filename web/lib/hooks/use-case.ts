import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { NewCaseEvent } from "@/lib/api/client";

export function useCase(caseId: string) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["case", tenantId, caseId],
    queryFn: () => api.getCase(tenantId, caseId),
    enabled: Boolean(caseId),
  });
}

export function useAppendCaseEvent(caseId: string) {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (event: NewCaseEvent) =>
      api.appendCaseEvent(tenantId, caseId, event),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["case", tenantId, caseId] });
      queryClient.invalidateQueries({ queryKey: ["breaks", tenantId] });
      queryClient.invalidateQueries({ queryKey: ["dashboard", tenantId] });
    },
  });
}
