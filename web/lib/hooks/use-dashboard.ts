import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";

export function useDashboard() {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["dashboard", tenantId],
    queryFn: () => api.getDashboard(tenantId),
  });
}
