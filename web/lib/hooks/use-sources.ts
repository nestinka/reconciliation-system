import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";

export function useSources(includeArchived = false) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["sources", tenantId, includeArchived],
    queryFn: () => api.listSources(tenantId, includeArchived),
  });
}
