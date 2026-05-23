import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { BreakQuery } from "@/lib/api/client";

export function useBreaks(q?: BreakQuery) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["breaks", tenantId, q],
    queryFn: () => api.listBreaks(tenantId, q),
  });
}

export function useAssignBreak() {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: ({
      breakId,
      userId,
    }: {
      breakId: string;
      userId: string;
    }) => api.assignBreak(tenantId, breakId, userId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["breaks", tenantId] });
      queryClient.invalidateQueries({ queryKey: ["dashboard", tenantId] });
    },
  });
}
