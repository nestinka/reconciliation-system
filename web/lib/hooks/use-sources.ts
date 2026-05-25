import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";

export function useSources() {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["sources", tenantId],
    queryFn: () => api.listSources(tenantId),
  });
}
